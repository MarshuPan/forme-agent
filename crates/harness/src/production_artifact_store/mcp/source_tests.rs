use super::super::source::ProtectedMcpCatalogSource;
use super::*;
use crate::production_artifact_store::worker_fixture::scope;
use forme_capabilities::ManagedMcpCatalogSource;
use forme_execution::installed::ManagedMcpSourceVerifier;
use forme_store::EventStore;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize},
    Mutex,
};

struct Authority {
    expected: p::WorkerResultManifest,
    settings: Mutex<p::ProductMcpServerConfig>,
    available: AtomicBool,
    reads: AtomicUsize,
    on_recheck: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl ManagedMcpSourceVerifier for Authority {
    fn verify_source(
        &self,
        manifest: &p::WorkerResultManifest,
    ) -> p::Result<p::ProductMcpServerConfig> {
        if self.reads.fetch_add(1, Ordering::SeqCst) > 0 {
            if let Some(change) = self.on_recheck.lock().unwrap().take() {
                change();
            }
        }
        if !self.available.load(Ordering::SeqCst) || *manifest != self.expected {
            return Err(p::Error("fixture source no longer current".into()));
        }
        Ok(self.settings.lock().unwrap().clone())
    }
}
fn setup(f: &Fixture) -> (Original, Arc<Authority>, Vec<p::EventPayload>) {
    let registration = p::canonical_digest(&"source registration").unwrap();
    let settings = p::ProductMcpServerConfig {
        schema_version: p::SchemaVersion(1),
        server_ref: "fixture-material".into(),
        program_ref: format!("mcp-program:{}", &registration.0[7..]),
        scope: scope(),
        enabled: true,
        trust: p::ProductSkillTrust::OwnerInput,
        timeout: p::DurationMs(1000),
        output_budget: 8192,
        allowlist: p::ProductMcpAllowlist {
            schema_version: p::SchemaVersion(1),
            tools: vec!["fixture.read".into()],
            resources: vec![],
        },
    };
    static NEXT_SOURCE: AtomicUsize = AtomicUsize::new(0);
    let mut action = intent();
    action.intent_id = p::ActionId(format!(
        "action:source:{}",
        NEXT_SOURCE.fetch_add(1, Ordering::SeqCst)
    ));
    let p::ActionParameters::McpDiscovery(spec) = &mut action.parameters else {
        panic!()
    };
    spec.program_ref = settings.program_ref.clone();
    spec.registration_digest = registration;
    spec.settings_digest = p::managed_mcp_settings_digest(&settings).unwrap();
    let (worker, transport) = backend(f.sink.clone());
    let plan = worker.plan(&action).unwrap();
    let sink = EventSink::default();
    let result = worker
        .execute(plan.clone(), &sink, CancelToken::default())
        .unwrap();
    assert_eq!(result.status, ActionStatus::Completed);
    let original = transport.original();
    let approval = p::ApprovalId("approval:source".into());
    let mut events = vec![
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: p::Source::OwnerControl,
            session_ref: p::SessionId("session:source".into()),
            input_ref: p::InputRef("input:source".into()),
            idempotency_key: Some(p::IdempotencyKey(format!("command:{}", action.intent_id.0))),
        }),
        p::EventPayload::SessionBound(p::SessionBoundPayload {
            policy_profile: p::PolicyProfileRef("policy:source".into()),
            model_profile: p::ModelProfileRef("model:source".into()),
            toolset_ref: p::ToolsetRef("tools:source".into()),
            workspace: p::WorkspaceRef(scope().0),
            effect_mode: None,
            evolution_snapshot: None,
            federation_snapshot: None,
            composition_manifest: Some(p::canonical_digest(&"source composition").unwrap()),
        }),
        p::EventPayload::ApprovalRequested(p::ApprovalRequestedPayload {
            approval_id: approval.clone(),
            action_summary: p::ActionSummary("Discover fixture".into()),
            risk: p::Risk::High,
            scope: scope(),
            rollback_boundary: plan.rollback_boundary.clone(),
            expires_at: now() + 60_000,
            choices: vec![p::ApprovalChoice("approve_once".into())],
            requested_permissions: action.requested_permissions.clone(),
            affected_resources: plan.affected_resources.clone(),
            plan_digest: Some(plan.digest.clone()),
            normalized_action_digest: Some(plan.normalized_action_digest.clone()),
            policy_version: Some(p::Version(1)),
            tool_schema_version: Some(p::Version(1)),
        }),
        p::EventPayload::ApprovalResolved(p::ApprovalResolvedPayload {
            approval_id: approval.clone(),
            outcome: p::ApprovalOutcome::Granted,
            grant_ref: Some(p::ApprovalGrantRef("grant:source".into())),
        }),
        p::EventPayload::ActionPlanned(p::ActionPlannedPayload {
            intent_id: action.intent_id,
            plan_digest: plan.digest,
            backend: p::BackendKind::Mcp,
            expected_effect: p::ExpectedEffect::Outward,
            source: p::Source::OwnerControl,
            scope: scope(),
            approval_ref: Some(approval),
            remote_placement: None,
        }),
    ];
    events.extend(sink.events());
    events.push(p::EventPayload::McpDiscovered(p::McpDiscoveredPayload {
        server: server(),
        tools: vec![p::ToolRef("fixture.read".into())],
        resources: vec![],
    }));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    let authority = Arc::new(Authority {
        expected: original.manifest.clone(),
        settings: Mutex::new(settings),
        available: AtomicBool::new(true),
        reads: AtomicUsize::new(0),
        on_recheck: Mutex::new(None),
    });
    (original, authority, events)
}
fn append(store: &forme_store::SqliteEventStore, run: &str, events: &[p::EventPayload]) {
    append_with_completion_provenance(store, run, events, crate::untrusted_action_provenance());
}
fn append_with_completion_provenance(
    store: &forme_store::SqliteEventStore,
    run: &str,
    events: &[p::EventPayload],
    completion: p::Provenance,
) {
    let run = p::RunId(run.into());
    for payload in events {
        let next = store.run_heads().unwrap().0.get(&run).copied().unwrap_or(0) + 1;
        store
            .append(p::Event::new(
                p::EventId(format!("source:{run:?}:{next}")),
                run.clone(),
                None,
                payload.clone(),
                p::SchemaVersion(1),
                now(),
                if crate::is_external_observation(payload) {
                    completion.clone()
                } else {
                    crate::internal_provenance(p::Source::OwnerControl)
                },
            ))
            .unwrap();
    }
}

#[test]
fn source_preserves_harness_external_completion_provenance() {
    for completion in [
        crate::untrusted_action_provenance(),
        crate::internal_provenance(p::Source::OwnerControl),
        crate::internal_provenance(p::Source::Internal),
    ] {
        let f = Fixture::new();
        let (_, authority, events) = setup(&f);
        append_with_completion_provenance(&f.store, "run:provenance", &events, completion.clone());
        assert_eq!(
            source(&f, authority).read_catalog(&server()).is_ok(),
            completion == crate::untrusted_action_provenance()
        );
    }
}
fn source(f: &Fixture, authority: Arc<dyn ManagedMcpSourceVerifier>) -> ProtectedMcpCatalogSource {
    ProtectedMcpCatalogSource {
        store: f.store.clone(),
        owner: owner(),
        authority,
    }
}
fn server() -> p::McpServerRef {
    p::McpServerRef("fixture-material".into())
}

#[test]
fn source_reopens_originals_without_events_keys_or_dispatch() {
    let f = Fixture::new();
    let (_, authority, events) = setup(&f);
    append(&f.store, "run:source", &events);
    let heads = f.store.run_heads().unwrap();
    let allocations = f.keys.allocations.load(Ordering::SeqCst);
    let catalog = source(&f, authority.clone())
        .read_catalog(&server())
        .unwrap();
    assert_eq!(
        catalog.1.tools[0].description,
        "untrusted fixture description"
    );
    assert_eq!(f.keys.allocations.load(Ordering::SeqCst), allocations);
    assert_eq!(f.store.run_heads().unwrap().0, heads.0);
    let root = f.root.clone();
    drop(f);
    let reopened = Fixture::open(root);
    assert_eq!(
        source(&reopened, authority)
            .read_catalog(&server())
            .unwrap(),
        catalog
    );
    assert_eq!(reopened.keys.allocations.load(Ordering::SeqCst), 0);
    assert_eq!(reopened.store.run_heads().unwrap().0, heads.0);
}

#[test]
fn source_requires_every_original_event_and_exact_binding() {
    for cut in 0..8 {
        let f = Fixture::new();
        let (_, authority, mut events) = setup(&f);
        let position = events
            .iter()
            .position(|e| match cut {
                0 => matches!(e, p::EventPayload::RunAccepted(_)),
                1 => matches!(e, p::EventPayload::SessionBound(_)),
                2 => matches!(e, p::EventPayload::ApprovalRequested(_)),
                3 => matches!(e, p::EventPayload::ApprovalResolved(_)),
                4 => matches!(e, p::EventPayload::ActionPlanned(_)),
                5 => matches!(e, p::EventPayload::ActionStarted(_)),
                6 => matches!(e, p::EventPayload::ActionCompleted(_)),
                _ => matches!(e, p::EventPayload::McpDiscovered(_)),
            })
            .unwrap();
        events.remove(position);
        append(&f.store, "run:missing", &events);
        assert!(
            source(&f, authority).read_catalog(&server()).is_err(),
            "cut {cut}"
        );
    }
}

#[test]
fn source_rejects_substituted_scope_plan_approval_receipt_and_composition() {
    for field in 0..11 {
        let f = Fixture::new();
        let (_, authority, mut events) = setup(&f);
        for event in &mut events {
            match (field, event) {
                (0, p::EventPayload::RunAccepted(v)) => v.source = p::Source::UserTurn,
                (1, p::EventPayload::SessionBound(v)) => v.workspace.0 = "workspace:foreign".into(),
                (2, p::EventPayload::SessionBound(v)) => v.composition_manifest = None,
                (3, p::EventPayload::ApprovalRequested(v)) => {
                    v.normalized_action_digest = Some(p::canonical_digest(&"wrong").unwrap())
                }
                (4, p::EventPayload::ApprovalResolved(v)) => v.outcome = p::ApprovalOutcome::Denied,
                (5, p::EventPayload::ActionPlanned(v)) => {
                    v.plan_digest = p::PlanDigest("other".into())
                }
                (6, p::EventPayload::ActionCompleted(v)) => {
                    v.receipt.as_mut().unwrap().probe_hint =
                        Some(p::ProbeHintRef("worker:other".into()))
                }
                (7, p::EventPayload::ActionCompleted(v)) => {
                    v.receipt.as_mut().unwrap().trust = p::TrustTier::OwnerInput
                }
                (8, p::EventPayload::ActionCompleted(v)) => {
                    v.receipt.as_mut().unwrap().content_digest =
                        Some(p::canonical_digest(&"other").unwrap())
                }
                (9, p::EventPayload::McpDiscovered(v)) => {
                    v.tools.push(p::ToolRef("not-allowed".into()))
                }
                (10, p::EventPayload::McpDiscovered(v)) => v
                    .resources
                    .push(p::ResourceRef("fixture://not-allowed".into())),
                _ => (),
            }
        }
        append(&f.store, "run:substituted", &events);
        assert!(
            source(&f, authority).read_catalog(&server()).is_err(),
            "field {field}"
        );
    }
}

#[test]
fn source_rejects_contradictory_completion_and_cross_run_reuse() {
    for duplicate in [false, true] {
        let f = Fixture::new();
        let (_, authority, events) = setup(&f);
        append(&f.store, "run:original", &events);
        if duplicate {
            append(&f.store, "run:other", &events);
        } else {
            let completed = events
                .iter()
                .find(|e| matches!(e, p::EventPayload::ActionCompleted(_)))
                .unwrap();
            append(&f.store, "run:original", std::slice::from_ref(completed));
        }
        assert!(source(&f, authority).read_catalog(&server()).is_err());
    }
}

#[test]
fn source_rechecks_current_authority_and_never_uses_revoked_cache() {
    let f = Fixture::new();
    let (_, authority, events) = setup(&f);
    append(&f.store, "run:source", &events);
    let reader = source(&f, authority.clone());
    reader.read_catalog(&server()).unwrap();
    authority.available.store(false, Ordering::SeqCst);
    assert!(reader.read_catalog(&server()).is_err());
    authority.available.store(true, Ordering::SeqCst);
    authority.settings.lock().unwrap().allowlist.tools.clear();
    assert!(reader.read_catalog(&server()).is_err());
    let mut other = source(&f, authority);
    other.owner = p::VerifiedPrincipal("owner:foreign".into());
    assert!(other.read_catalog(&server()).is_err());
}

#[test]
fn source_respects_pending_and_completed_deletion_fences() {
    for interrupted in [false, true] {
        let f = Fixture::new();
        let (_, authority, events) = setup(&f);
        append(&f.store, "run:source", &events);
        source(&f, authority.clone())
            .read_catalog(&server())
            .unwrap();
        let lifecycle = f.protection.lifecycle_service(f.store.clone());
        let request = f.deletion();
        let plan = lifecycle.plan(&request).unwrap();
        f.keys.fail_destroy.store(interrupted, Ordering::SeqCst);
        assert_eq!(
            lifecycle.apply_plan(request, &plan.digest).is_err(),
            interrupted
        );
        assert!(source(&f, authority).read_catalog(&server()).is_err());
    }
}

#[test]
fn source_uses_host_admission_order_instead_of_run_id_or_time() {
    let f = Fixture::new();
    let (_, old, events) = setup(&f);
    append(&f.store, "run:zz-old", &events);
    // A separate completed original can only become current through the Host.
    let (_, current, events) = setup(&f);
    append(&f.store, "run:aa-new", &events);
    assert_ne!(old.expected.binding.job_id, current.expected.binding.job_id);
    let catalog = source(&f, current).read_catalog(&server()).unwrap();
    assert_eq!(catalog.1.spec.server, server());
}

#[test]
fn source_detects_deletion_during_host_recheck() {
    let f = Fixture::new();
    let (_, authority, events) = setup(&f);
    append(&f.store, "run:source", &events);
    let lifecycle = f.protection.lifecycle_service(f.store.clone());
    let request = f.deletion();
    let plan = lifecycle.plan(&request).unwrap();
    *authority.on_recheck.lock().unwrap() = Some(Box::new(move || {
        lifecycle.apply_plan(request, &plan.digest).unwrap();
    }));
    assert!(source(&f, authority).read_catalog(&server()).is_err());
}

#[test]
fn source_snapshot_ignores_unrelated_runs_but_rejects_changed_original() {
    for unrelated in [false, true] {
        let f = Fixture::new();
        let (original, authority, events) = setup(&f);
        append(&f.store, "run:source", &events);
        let store = f.store.clone();
        *authority.on_recheck.lock().unwrap() = Some(Box::new(move || {
            append(
                &store,
                if unrelated {
                    "run:unrelated"
                } else {
                    "run:source"
                },
                &[p::EventPayload::ActionOutcomeUnknown(
                    p::ActionOutcomeUnknownPayload {
                        intent_id: original.request.job.action_ref,
                        probe_hint: p::ProbeHintRef("worker:unknown".into()),
                        remote_lease: None,
                    },
                )],
            );
        }));
        assert_eq!(
            source(&f, authority).read_catalog(&server()).is_ok(),
            unrelated
        );
    }
}
