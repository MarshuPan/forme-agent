use super::*;
use crate::production_artifact_store::{
    mcp::tests::fixture::{backend, Transport},
    worker_fixture::{owner, scope, Fixture},
};
use forme_approval::ApprovalGrant;
use forme_policy::{ActionMatcher, ArgMatcher, PolicyLayer, PolicyLayerSource, PolicyRule};

fn settings() -> p::ProductMcpServerConfig {
    p::ProductMcpServerConfig {
        schema_version: p::SchemaVersion(1),
        server_ref: "fixture-material".into(),
        program_ref: format!("mcp-program:{}", "a".repeat(64)),
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
    }
}
fn spec() -> p::ManagedMcpDiscoverySpec {
    let s = settings();
    p::ManagedMcpDiscoverySpec {
        schema_version: p::SchemaVersion(1),
        server: p::McpServerRef(s.server_ref.clone()),
        program_ref: s.program_ref.clone(),
        registration_digest: p::SchemaDigest(format!("sha256:{}", "a".repeat(64))),
        settings_digest: p::managed_mcp_settings_digest(&s).unwrap(),
        timeout: s.timeout,
        output_budget_bytes: s.output_budget,
    }
}
struct Authority(Arc<Transport>);
impl forme_execution::installed::ManagedMcpSourceVerifier for Authority {
    fn verify_source(
        &self,
        manifest: &p::WorkerResultManifest,
    ) -> p::Result<p::ProductMcpServerConfig> {
        if self.0.original().manifest == *manifest {
            Ok(settings())
        } else {
            Err(invalid())
        }
    }
}
fn harness(f: &Fixture) -> (ReactiveHarness, Arc<Transport>) {
    harness_with_consumer(f, f.sink.clone())
}
fn harness_with_consumer(
    f: &Fixture,
    consumer: Arc<dyn forme_execution::installed::ManagedMcpDiscoveryConsumer>,
) -> (ReactiveHarness, Arc<Transport>) {
    let (worker, transport) = backend(consumer);
    harness_with_worker(f, worker, transport)
}
fn harness_with_worker(
    f: &Fixture,
    worker: forme_execution::installed::WorkerActionBackend,
    transport: Arc<Transport>,
) -> (ReactiveHarness, Arc<Transport>) {
    let worker = Arc::new(
        worker.with_discovery_dispatch_recorder(original_job::recorder(
            f.store.clone(),
            owner(),
            scope(),
            p::canonical_digest(&"owner-discovery-worker").unwrap(),
            p::AuthorityEpoch(1),
        )),
    );
    let registry = Arc::new(ExecutionBackendRegistry::default());
    registry.register(worker.clone()).unwrap();
    let profile = action_only_model_profile().unwrap();
    let model = Arc::new(ScriptedModelProvider::new(profile.clone(), vec![]).unwrap());
    let mut config = HarnessConfig::for_model(&profile);
    config.workspace = p::WorkspaceRef(scope().0);
    let mut harness = ReactiveHarness::new(
        f.store.clone(),
        model,
        registry,
        GovernanceConfig::default(),
        config,
    )
    .unwrap()
    .with_authority_guard(f._guard.clone())
    .with_competence_gate(Arc::new(cognition::EvidenceCompetenceGate::default()));
    harness.configured_owner = Some(owner());
    harness.composition_manifest = Some(p::canonical_digest(&"owner-discovery-fixture").unwrap());
    harness.owner_discovery = Some(DiscoveryRuntime {
        settings: vec![settings()],
        worker_digest: p::canonical_digest(&"owner-discovery-worker").unwrap(),
        authority: Arc::new(Authority(transport.clone())),
        backend: worker,
    });
    (harness, transport)
}
fn submit(h: &ReactiveHarness) -> p::RunId {
    h.discover_managed_mcp(
        &owner(),
        p::SessionId("session:owner-discovery".into()),
        p::IdempotencyKey("command:owner-discovery".into()),
        spec(),
    )
    .unwrap()
}
fn grant(h: &ReactiveHarness) -> ApprovalGrant {
    let pending = h
        .pending_approvals(p::SessionId("session:owner-discovery".into()))
        .unwrap();
    assert_eq!(pending.len(), 1);
    let a = &pending[0];
    ApprovalGrant {
        schema_version: p::SchemaVersion(1),
        approval_id: a.approval_id.clone(),
        outcome: p::ApprovalOutcome::Granted,
        granted_scope: GrantScope::OneShot,
        approver: owner(),
        bound_plan_digest: a.plan_digest.clone(),
        normalized_action_digest: a.normalized_action_digest.clone(),
        affected_resources: a.affected_resources.clone(),
        policy_version: a.policy_version,
        tool_schema_version: a.tool_schema_version,
        nonce: p::Nonce("nonce:owner-discovery".into()),
        use_by: a.expires_at - 1,
    }
}
fn events(h: &ReactiveHarness, run: &p::RunId) -> Vec<p::Event> {
    h.store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
}
fn assert_cold(events: &[p::Event], count: usize) {
    let ceilings = events
        .iter()
        .filter_map(|e| match &e.payload {
            p::EventPayload::CompetenceGateEvaluated(v) => Some(v.max_level),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ceilings.len(), count);
    assert!(ceilings
        .iter()
        .all(|level| *level == p::InterventionLevel::L0Observe));
    assert!(!events
        .iter()
        .any(|e| e.kind == p::EventKind::ModelCallStarted));
    assert!(!events
        .iter()
        .any(|e| e.kind == p::EventKind::CapabilityEvidenceRecorded));
}

#[test]
fn real_cold_gate_records_l0_and_exact_approval_dispatches_once() {
    let f = Fixture::new();
    let (h, transport) = harness(&f);
    let run = submit(&h);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    assert_cold(&events(&h, &run), 1);
    let approval = grant(&h);
    h.resume(run.clone(), ResumeInput::Approval(approval.clone()))
        .unwrap();
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    let events = events(&h, &run);
    assert_cold(&events, 2);
    assert!(events
        .iter()
        .any(|e| e.kind == p::EventKind::ActionCompleted));
    assert!(events.iter().any(|e| e.kind == p::EventKind::RunComplete));
    assert!(h
        .resume(run.clone(), ResumeInput::Approval(approval))
        .is_err());
    assert_eq!(submit(&h), run);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn pending_restart_restores_exact_original_without_events_models_or_dispatch() {
    let f = Fixture::new();
    let (h, transport) = harness(&f);
    let run = submit(&h);
    let approval = grant(&h);
    let before = events(&h, &run);
    drop(h);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    let (h, transport) = harness(&f);
    h.recover_owner_discoveries().unwrap();
    assert_eq!(events(&h, &run), before);
    assert_eq!(grant(&h), approval);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    h.resume(run.clone(), ResumeInput::Approval(approval))
        .unwrap();
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert!(events(&h, &run)
        .iter()
        .any(|e| e.kind == p::EventKind::RunComplete));
}

#[test]
fn public_source_claim_and_foreign_principal_cannot_create_private_owner_binding() {
    let f = Fixture::new();
    let (h, transport) = harness(&f);
    assert!(h
        .discover_managed_mcp(
            &p::VerifiedPrincipal("owner:other".into()),
            p::SessionId("session:test".into()),
            p::IdempotencyKey("command:test".into()),
            spec()
        )
        .is_err());
    assert!(f.store.run_ids().unwrap().is_empty());
    let run = submit(&h);
    let original: OwnerRequest = read(&h, &run, "request").unwrap().unwrap();
    for source in [
        p::Source::OwnerControl,
        p::Source::UserTurn,
        p::Source::Schedule,
        p::Source::ProactiveJob,
    ] {
        let mut request = original.request.clone();
        request.source = source;
        request.idempotency_key = Some(p::IdempotencyKey(format!("forged:{source:?}")));
        let mut intent = original.intent.clone();
        intent.source = source;
        if let Ok(forged) = h.submit_action(request, intent, original.envelope.clone(), Vec::new())
        {
            assert!(events(&h, &forged)
                .iter()
                .all(|e| e.kind != p::EventKind::ApprovalRequested
                    && e.kind != p::EventKind::ActionStarted));
            assert!(read::<OwnerRequest>(&h, &forged, "request")
                .unwrap()
                .is_none());
        }
    }
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn deny_and_configuration_drift_cannot_be_overridden_by_exact_owner_approval() {
    for change in 0..4 {
        let f = Fixture::new();
        let (mut h, transport) = harness(&f);
        let run = submit(&h);
        let approval = grant(&h);
        if change == 0 {
            h.governance.layers.push(PolicyLayer {
                schema_version: p::SchemaVersion(1),
                source: PolicyLayerSource::Managed,
                rules: vec![PolicyRule {
                    schema_version: p::SchemaVersion(1),
                    matcher: ActionMatcher {
                        parameters: ArgMatcher::Any,
                        ..Default::default()
                    },
                    effect: p::PolicyDecision::Deny,
                    scope: scope(),
                }],
            });
        } else if change == 1 {
            h.owner_discovery.as_mut().unwrap().settings[0].enabled = false;
        } else {
            let mut envelope = read::<OwnerRequest>(&h, &run, "request")
                .unwrap()
                .unwrap()
                .envelope;
            envelope.approval_rule = p::ApprovalRule::Deny;
            if change == 2 {
                h.governance.envelope = Some(envelope);
            } else {
                h.governance.delegation = Some(forme_policy::DelegationGrant {
                    schema_version: p::SchemaVersion(1),
                    subject: forme_policy::DelegationSubject::Owner,
                    envelope,
                    granted_by: p::Actor::Owner,
                    audit_ref: events(&h, &run)[0].event_id.clone(),
                });
            }
        }
        let _ = h.resume(run.clone(), ResumeInput::Approval(approval));
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
        assert!(!events(&h, &run)
            .iter()
            .any(|e| e.kind == p::EventKind::ActionStarted));
    }
}

struct LostMaterial;
impl forme_execution::installed::ManagedMcpDiscoveryConsumer for LostMaterial {
    fn persist_discovery(
        &self,
        _: &p::WorkerDispatchRequest,
        _: &p::WorkerObservation,
        _: &p::WorkerResultManifest,
        _: &[u8],
    ) -> p::Result<forme_execution::ArtifactRecord> {
        Err(p::Error("fixture lost discovery material".into()))
    }
}

#[test]
fn unknown_discovery_cannot_be_manually_completed_or_reexecuted_after_restart() {
    let f = Fixture::new();
    let (h, transport) = harness_with_consumer(&f, Arc::new(LostMaterial));
    let run = submit(&h);
    h.resume(run.clone(), ResumeInput::Approval(grant(&h)))
        .unwrap();
    let before = events(&h, &run);
    let action = before
        .iter()
        .find_map(|e| match &e.payload {
            p::EventPayload::ActionOutcomeUnknown(v) => Some(v.intent_id.clone()),
            _ => None,
        })
        .unwrap();
    let manual = || {
        ResumeInput::ToolOutcome(
            action.clone(),
            ResolvedOutcome::Completed(p::ActionResultRef("result:claimed".into()), String::new()),
        )
    };
    assert!(h.resume(run.clone(), manual()).is_err());
    assert!(h.publish_managed_mcp(&owner(), run.clone()).is_err());
    assert_eq!(events(&h, &run), before);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    drop(h);
    let (h, next_transport) = harness(&f);
    h.recover_unknown_outcomes().unwrap();
    h.recover_owner_discoveries().unwrap();
    assert!(h.resume(run.clone(), manual()).is_err());
    assert!(h.publish_managed_mcp(&owner(), run.clone()).is_err());
    assert_eq!(events(&h, &run), before);
    assert_eq!(next_transport.calls.load(Ordering::SeqCst), 0);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn interrupted_or_resolved_pending_is_never_recreated_after_restart() {
    for resolved in [true, false] {
        let f = Fixture::new();
        let (h, transport) = harness(&f);
        let run = submit(&h);
        let original: OwnerRequest = read(&h, &run, "request").unwrap().unwrap();
        if resolved {
            let a = grant(&h);
            let handle = h.run_handle(&run).unwrap();
            let record = handle.record.lock().unwrap();
            record
                .approval
                .resolve(ApprovalTicket(a.approval_id.clone()), a)
                .unwrap();
            for payload in record.approval.take_events() {
                h.append(
                    run.clone(),
                    None,
                    internal_provenance(p::Source::OwnerControl),
                    payload,
                )
                .unwrap();
            }
        } else {
            // A different worker identity invalidates an otherwise intact original.
            assert_eq!(
                original.worker,
                h.owner_discovery.as_ref().unwrap().worker_digest
            );
        }
        drop(h);
        let (mut h, _) = harness(&f);
        if !resolved {
            h.owner_discovery.as_mut().unwrap().worker_digest =
                p::canonical_digest(&"new authority").unwrap();
        }
        h.recover_owner_discoveries().unwrap();
        assert!(h.run_handle(&run).is_err());
        assert!(events(&h, &run)
            .iter()
            .any(|e| e.kind == p::EventKind::RunFailed));
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    }
}

struct UnavailableAuthority;
impl forme_execution::installed::ManagedMcpSourceVerifier for UnavailableAuthority {
    fn verify_source(&self, _: &p::WorkerResultManifest) -> p::Result<p::ProductMcpServerConfig> {
        Err(p::Error("fixture source temporarily unavailable".into()))
    }
}

#[test]
fn completed_discovery_recovers_publication_without_reexecuting_or_contradicting_completion() {
    let f = Fixture::new();
    let (mut h, transport) = harness(&f);
    h.owner_discovery.as_mut().unwrap().authority = Arc::new(UnavailableAuthority);
    let run = submit(&h);
    h.resume(run.clone(), ResumeInput::Approval(grant(&h)))
        .unwrap();
    let first = events(&h, &run);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        first
            .iter()
            .filter(|e| e.kind == p::EventKind::ActionCompleted)
            .count(),
        1
    );
    assert!(!first.iter().any(|e| matches!(
        e.kind,
        p::EventKind::ActionFailed
            | p::EventKind::ActionDenied
            | p::EventKind::ActionOutcomeUnknown
            | p::EventKind::RunComplete
            | p::EventKind::McpDiscovered
    )));
    drop(h);
    let (mut h, new_transport) = harness(&f);
    h.owner_discovery.as_mut().unwrap().authority = Arc::new(Authority(transport.clone()));
    h.recover_owner_discoveries().unwrap();
    assert_eq!(events(&h, &run), first);
    h.publish_managed_mcp(&owner(), run.clone()).unwrap();
    h.publish_managed_mcp(&owner(), run.clone()).unwrap();
    let final_events = events(&h, &run);
    for kind in [
        p::EventKind::ActionCompleted,
        p::EventKind::McpDiscovered,
        p::EventKind::RunComplete,
    ] {
        assert_eq!(final_events.iter().filter(|e| e.kind == kind).count(), 1);
    }
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert_eq!(new_transport.calls.load(Ordering::SeqCst), 0);
    assert!(!final_events
        .iter()
        .any(|e| e.kind == p::EventKind::ModelCallStarted));
}

#[test]
fn original_request_and_pending_are_encrypted_and_lifecycle_deletion_blocks_approval() {
    let f = Fixture::new();
    let (h, transport) = harness(&f);
    let run = submit(&h);
    let approval = grant(&h);
    let original: OwnerRequest = read(&h, &run, "request").unwrap().unwrap();
    let bytes = serde_json::to_vec(&original).unwrap();
    let db = std::fs::read(f.root.0.join("brain.db")).unwrap();
    assert!(!db.windows(bytes.len()).any(|part| part == bytes));
    let lifecycle = f.protection.lifecycle_service(f.store.clone());
    let request = f.deletion();
    let plan = lifecycle.plan(&request).unwrap();
    lifecycle.apply_plan(request, &plan.digest).unwrap();
    assert!(read::<OwnerRequest>(&h, &run, "request").unwrap().is_none());
    let _ = h.resume(run.clone(), ResumeInput::Approval(approval));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn altered_nonce_binding_and_expired_approval_never_reach_the_worker() {
    for change in 0..4 {
        let f = Fixture::new();
        let (h, transport) = harness(&f);
        let run = submit(&h);
        let mut approval = grant(&h);
        match change {
            0 => approval.approver = p::VerifiedPrincipal("owner:other".into()),
            1 => {
                approval.bound_plan_digest =
                    p::PlanDigest(p::canonical_digest(&"other plan").unwrap().0)
            }
            2 => approval.policy_version.0 += 1,
            _ => approval.use_by = now_ms() - 1,
        }
        let _ = h.resume(run.clone(), ResumeInput::Approval(approval));
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
        assert!(!events(&h, &run)
            .iter()
            .any(|e| e.kind == p::EventKind::ActionStarted));
    }
}

#[test]
fn rejected_discovery_is_not_reported_as_a_successful_publication() {
    let f = Fixture::new();
    let (h, transport) = harness(&f);
    let run = submit(&h);
    let mut approval = grant(&h);
    approval.outcome = p::ApprovalOutcome::Denied;
    h.resume(run.clone(), ResumeInput::Approval(approval))
        .unwrap();
    let before = events(&h, &run);
    assert!(h.publish_managed_mcp(&owner(), run.clone()).is_err());
    assert_eq!(events(&h, &run), before);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[path = "verification_tests.rs"]
mod verification_tests;
