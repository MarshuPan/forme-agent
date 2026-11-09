use super::*;
use crate::production_artifact_store::worker_fixture::*;
use crate::production_artifact_store::ProtectedRuntimeArtifacts;
use forme_execution::{ArtifactStore, FileArtifactStore};

fn query() -> p::ToolArtifactCatalogQuery {
    p::ToolArtifactCatalogQuery {
        schema_version: p::TOOL_ARTIFACT_SCHEMA_VERSION,
        session: p::SessionId("session:tool-images".into()),
        run: p::RunId("run:tool-images".into()),
        page: p::DataPageQuery {
            schema_version: p::SchemaVersion(1),
            offset: 0,
            limit: 1,
            snapshot: None,
        },
    }
}
fn append(f: &Fixture, payload: p::EventPayload) {
    append_to(&f.store, payload);
}
fn append_to(store: &SqliteEventStore, payload: p::EventPayload) {
    let run = query().run;
    let next = store.run_heads().unwrap().0.get(&run).copied().unwrap_or(0) + 1;
    store
        .append(p::Event::new(
            p::EventId(format!("tool-image-event:{next}")),
            run,
            None,
            payload,
            p::SchemaVersion(1),
            now(),
            p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            },
        ))
        .unwrap();
}
fn prepare(f: &Fixture, binding: &p::WorkerArtifactBinding) {
    prepare_store(&f.store, binding);
}
fn prepare_store(store: &SqliteEventStore, binding: &p::WorkerArtifactBinding) {
    append_to(
        store,
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: p::Source::UserTurn,
            session_ref: query().session,
            input_ref: p::InputRef("input:images".into()),
            idempotency_key: None,
        }),
    );
    append_to(
        store,
        p::EventPayload::SessionBound(p::SessionBoundPayload {
            policy_profile: p::PolicyProfileRef("policy:images".into()),
            model_profile: p::ModelProfileRef("model:images".into()),
            toolset_ref: p::ToolsetRef("toolset:images".into()),
            workspace: p::WorkspaceRef(scope().0),
            effect_mode: None,
            evolution_snapshot: None,
            federation_snapshot: None,
            composition_manifest: None,
        }),
    );
    append_to(
        store,
        p::EventPayload::ActionPlanned(p::ActionPlannedPayload {
            intent_id: binding.manifest.binding.action_ref.clone(),
            plan_digest: binding.manifest.binding.plan_digest.clone(),
            backend: p::BackendKind::Computer,
            expected_effect: p::ExpectedEffect::Internal,
            source: p::Source::Internal,
            scope: scope(),
            approval_ref: None,
            remote_placement: None,
        }),
    );
    append_to(
        store,
        p::EventPayload::ActionStarted(p::ActionStartedPayload {
            intent_id: binding.manifest.binding.action_ref.clone(),
            backend: p::BackendKind::Computer,
            scope: scope(),
            remote_lease: None,
        }),
    );
}
fn completed(binding: &p::WorkerArtifactBinding, id: p::ContentRef) -> p::ActionCompletedPayload {
    let action = binding.manifest.binding.action_ref.clone();
    p::ActionCompletedPayload {
        intent_id: action.clone(),
        result_ref: p::ActionResultRef("result:images".into()),
        receipt: Some(p::ExternalActionReceipt {
            schema_version: p::SchemaVersion(1),
            action,
            content_ref: Some(id),
            content_digest: Some(binding.descriptor.content_digest.clone()),
            trust: p::TrustTier::Untrusted,
            effect: p::EffectStatus::Observed,
            probe_hint: Some(p::ProbeHintRef(format!(
                "worker:{}",
                binding.manifest.binding.job_id
            ))),
        }),
        remote_receipt: None,
    }
}
fn ready() -> (Fixture, p::WorkerArtifactBinding, p::ContentRef) {
    let f = Fixture::new();
    let binding = material();
    prepare(&f, &binding);
    let id = f
        .sink
        .write_worker_output(&binding, &png())
        .unwrap()
        .content_ref;
    append(
        &f,
        p::EventPayload::ActionCompleted(completed(&binding, id.clone())),
    );
    (f, binding, id)
}
fn read_request(id: p::ContentRef, binding: &p::WorkerArtifactBinding) -> p::ToolArtifactRead {
    p::ToolArtifactRead {
        schema_version: p::TOOL_ARTIFACT_SCHEMA_VERSION,
        session: query().session,
        run: query().run,
        content_ref: id,
        content_digest: binding.descriptor.content_digest.clone(),
        offset: 0,
        max_bytes: 13,
    }
}
#[test]
fn completed_original_reads_in_chunks_across_restart_without_new_facts_or_keys() {
    let (f, binding, id) = ready();
    let view = query_original(&f.store, &owner(), query()).unwrap();
    assert_eq!(view.total, 1);
    assert_eq!(view.artifacts[0].content_ref, id);
    assert_eq!(view.artifacts[0].width, 2);
    let heads = f.store.run_heads().unwrap();
    let allocations = f.keys.allocations.load(std::sync::atomic::Ordering::SeqCst);
    let mut request = read_request(id.clone(), &binding);
    let mut bytes = Vec::<u8>::new();
    loop {
        let chunk = read_original(&f.store, &owner(), request.clone()).unwrap();
        bytes.extend(&chunk.bytes);
        if chunk.finished {
            break;
        }
        request.offset = chunk.next_offset;
    }
    assert_eq!(bytes, png());
    assert_eq!(f.store.run_heads().unwrap(), heads);
    assert_eq!(
        f.keys.allocations.load(std::sync::atomic::Ordering::SeqCst),
        allocations
    );
    let root = f.root.clone();
    drop(f);
    let f = Fixture::open(root);
    assert_eq!(query_original(&f.store, &owner(), query()).unwrap(), view);
    assert!(read_original(&f.store, &owner(), read_request(id, &binding)).is_ok());
    assert_eq!(
        f.keys.allocations.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}
#[test]
fn foreign_session_run_owner_digest_and_range_cannot_read_original() {
    let (f, binding, id) = ready();
    let request = read_request(id, &binding);
    for mode in 0..6 {
        let mut wrong = request.clone();
        let mut who = owner();
        match mode {
            0 => wrong.session = p::SessionId("session:foreign".into()),
            1 => wrong.run = p::RunId("run:foreign".into()),
            2 => who = p::VerifiedPrincipal("owner:foreign".into()),
            3 => wrong.content_digest = p::sha256_content_digest(b"wrong"),
            4 => wrong.offset = binding.descriptor.bytes,
            _ => {
                wrong.content_ref =
                    p::ContentRef(format!("content:{}", p::sha256_content_digest(b"other").0))
            }
        };
        assert!(read_original(&f.store, &who, wrong).is_err());
    }
    let mut wrong = query();
    wrong.session = p::SessionId("session:foreign".into());
    assert!(query_original(&f.store, &owner(), wrong).is_err());
}
#[test]
fn pending_reference_only_unknown_and_deleted_objects_never_enter_preview_catalog() {
    let f = Fixture::new();
    let binding = material();
    prepare(&f, &binding);
    let id = f
        .sink
        .write_worker_output(&binding, &png())
        .unwrap()
        .content_ref;
    assert_eq!(
        query_original(&f.store, &owner(), query()).unwrap().total,
        0
    );
    assert!(read_original(&f.store, &owner(), read_request(id.clone(), &binding)).is_err());
    append(
        &f,
        p::EventPayload::ActionCompleted(completed(&binding, id.clone())),
    );
    assert_eq!(
        query_original(&f.store, &owner(), query()).unwrap().total,
        1
    );
    let mut old = query();
    old.page.snapshot = Some(query_original(&f.store, &owner(), query()).unwrap().digest);
    assert!(f.body(&id).is_some());
    let service = f.protection.lifecycle_service(f.store.clone());
    let request = f.deletion();
    let plan = service.plan(&request).unwrap();
    service.apply_plan(request, &plan.digest).unwrap();
    assert!(f.body(&id).is_none());
    assert_eq!(
        query_original(&f.store, &owner(), query()).unwrap().total,
        0
    );
    assert!(query_original(&f.store, &owner(), old).is_err());
    assert!(read_original(&f.store, &owner(), read_request(id, &binding)).is_err());
}
#[test]
fn completed_receipt_requires_matching_original_plan_and_server_material_registration() {
    for mode in 0..4 {
        let f = Fixture::new();
        let binding = material();
        prepare(&f, &binding);
        let id = if mode == 0 {
            f.put("unregistered-png", &png()).content_ref
        } else {
            f.sink
                .write_worker_output(&binding, &png())
                .unwrap()
                .content_ref
        };
        let mut completion = completed(&binding, id.clone());
        let receipt = completion.receipt.as_mut().unwrap();
        match mode {
            1 => receipt.content_digest = Some(p::sha256_content_digest(b"different")),
            2 => receipt.probe_hint = Some(p::ProbeHintRef("worker:other".into())),
            3 => receipt.trust = p::TrustTier::OwnerInput,
            _ => (),
        };
        append(&f, p::EventPayload::ActionCompleted(completion));
        let view = query_original(&f.store, &owner(), query());
        if mode == 0 {
            assert_eq!(view.unwrap().total, 0);
        } else {
            assert!(view.is_err());
        }
        assert!(read_original(&f.store, &owner(), read_request(id, &binding)).is_err());
    }
}

#[test]
fn unknown_stays_hidden_until_original_completion_and_plan_mismatch_is_rejected() {
    let f = Fixture::new();
    let binding = material();
    prepare(&f, &binding);
    let id = f
        .sink
        .write_worker_output(&binding, &png())
        .unwrap()
        .content_ref;
    append(
        &f,
        p::EventPayload::ActionOutcomeUnknown(p::ActionOutcomeUnknownPayload {
            intent_id: binding.manifest.binding.action_ref.clone(),
            probe_hint: p::ProbeHintRef(format!("worker:{}", binding.manifest.binding.job_id)),
            remote_lease: None,
        }),
    );
    assert_eq!(
        query_original(&f.store, &owner(), query()).unwrap().total,
        0
    );
    append(
        &f,
        p::EventPayload::ActionCompleted(completed(&binding, id.clone())),
    );
    assert_eq!(
        query_original(&f.store, &owner(), query()).unwrap().total,
        1
    );
    assert!(read_original(&f.store, &owner(), read_request(id, &binding)).is_ok());
    let f = Fixture::new();
    let mut other = binding.clone();
    other.manifest.binding.plan_digest = p::PlanDigest("different-original-plan".into());
    prepare(&f, &other);
    let id = f
        .sink
        .write_worker_output(&binding, &png())
        .unwrap()
        .content_ref;
    append(
        &f,
        p::EventPayload::ActionCompleted(completed(&binding, id.clone())),
    );
    assert!(query_original(&f.store, &owner(), query()).is_err());
    assert!(read_original(&f.store, &owner(), read_request(id, &binding)).is_err());
}
#[test]
fn metadata_presence_obeys_scope_subject_and_pending_deletion_before_body_removal() {
    let (f, binding, id) = ready();
    let subject = p::sha256_content_digest(scope().0.as_bytes());
    assert!(f
        .store
        .protected_content_available_for_current_owner(&id, &scope(), &subject)
        .unwrap());
    assert!(!f
        .store
        .protected_content_available_for_current_owner(
            &id,
            &p::Scope("workspace:other".into()),
            &subject
        )
        .unwrap());
    assert!(!f
        .store
        .protected_content_available_for_current_owner(
            &id,
            &scope(),
            &p::sha256_content_digest(b"other")
        )
        .unwrap());
    let service = f.protection.lifecycle_service(f.store.clone());
    let request = f.deletion();
    let plan = service.plan(&request).unwrap();
    f.keys
        .fail_destroy
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(service.apply_plan(request.clone(), &plan.digest).is_err());
    assert!(!f
        .store
        .protected_content_available_for_current_owner(&id, &scope(), &subject)
        .unwrap());
    assert_eq!(
        query_original(&f.store, &owner(), query()).unwrap().total,
        0
    );
    assert!(read_original(&f.store, &owner(), read_request(id.clone(), &binding)).is_err());
    f.keys
        .fail_destroy
        .store(false, std::sync::atomic::Ordering::SeqCst);
    service.apply_plan(request, &plan.digest).unwrap();
    assert!(!f
        .store
        .protected_content_available_for_current_owner(&id, &scope(), &subject)
        .unwrap());
}
#[test]
fn production_reader_enforces_owner_and_data_fence_without_model_or_tool_activity() {
    let root = Directory::new();
    std::env::set_var(
        "FORME_TOOL_IMAGE_READER_FIXTURE_KEY",
        "self-authored-fixture-credential",
    );
    let config = crate::ProductionRuntimeConfig {
        schema_version: p::SchemaVersion(1),
        owner: p::OwnerPrincipalRef(owner().0),
        store_path: root.0.join("runtime.db"),
        workspace: p::WorkspaceRef(scope().0),
        agent_profile: p::AgentProfileRef("agent:tool-images".into()),
        models: vec![forme_models::ModelRuntimeConfig {
            id: "model:tool-images".into(),
            provider: "tool-image-fixture".into(),
            model: "fixture".into(),
            base_url: "http://127.0.0.1:9/v1".into(),
            credential_env: "FORME_TOOL_IMAGE_READER_FIXTURE_KEY".into(),
            context_window: 32768,
            tool_use: false,
            strength: forme_models::ModelStrength::Standard,
            input_microunits_per_million: 1,
            output_microunits_per_million: 1,
            requests_per_minute: 60,
            tokens_per_minute: 100000,
            timeout_ms: 1000,
            fallback_group: None,
            default: true,
            tool_schema_versions: vec![],
        }],
        capabilities: crate::CapabilityRuntimeConfig::default(),
        scheduler_enabled: false,
        scheduler: p::SchedulerConfig {
            schema_version: p::SchemaVersion(1),
            tick: p::DurationMs(1000),
            lease: p::DurationMs(30000),
            max_claims_per_tick: 1,
        },
        cognition: crate::CognitiveRuntimeConfig::default(),
        data: crate::DataProtectionConfig {
            schema_version: p::SchemaVersion(1),
            artifact_root: root.0.join("artifacts"),
        },
        authority_lease: p::DurationMs(60000),
        context_limits: forme_context::ContextHydrationLimits::default(),
    };
    let runtime = crate::ProductionRuntimeBuilder::from_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    let store = runtime.services().store.clone();
    let protection = store.content_protection().unwrap().unwrap();
    let sink = ProtectedRuntimeArtifacts {
        inner: FileArtifactStore::new(root.0.join("artifacts")).unwrap(),
        protection: protection.clone(),
        store: store.as_ref().clone(),
        owner: owner(),
    };
    let binding = material();
    prepare_store(&store, &binding);
    let id = sink
        .write_worker_output(&binding, &png())
        .unwrap()
        .content_ref;
    append_to(
        &store,
        p::EventPayload::ActionCompleted(completed(&binding, id.clone())),
    );
    append_to(
        &store,
        p::EventPayload::RunComplete(p::RunCompletePayload {
            stop_reason: p::StopReason("fixture completion".into()),
            result_ref: None,
        }),
    );
    let heads = store.run_heads().unwrap();
    let first = runtime.query_tool_artifacts(&owner(), query()).unwrap();
    assert_eq!(first.total, 1);
    assert!(runtime
        .query_tool_artifacts(&p::VerifiedPrincipal("owner:foreign".into()), query())
        .is_err());
    assert!(runtime
        .read_tool_artifact(
            &p::VerifiedPrincipal("owner:foreign".into()),
            read_request(id.clone(), &binding)
        )
        .is_err());
    assert!(runtime
        .read_tool_artifact(&owner(), read_request(id.clone(), &binding))
        .is_ok());
    assert_eq!(store.run_heads().unwrap(), heads);
    drop(sink);
    drop(protection);
    drop(store);
    drop(runtime);
    let runtime = crate::ProductionRuntimeBuilder::from_config(config)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(
        runtime.query_tool_artifacts(&owner(), query()).unwrap(),
        first
    );
    let services = runtime.services();
    let fence = services.data_access.write().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|threads| {
        threads.spawn(|| {
            tx.send(runtime.read_tool_artifact(&owner(), read_request(id, &binding)))
                .unwrap();
        });
        assert!(rx
            .recv_timeout(std::time::Duration::from_millis(50))
            .is_err());
        drop(fence);
        assert!(rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .is_ok());
    });
    assert_eq!(services.store.run_heads().unwrap(), heads);
}
