use super::*;
use crate::production_artifact_store::mcp::tests::fixture::backend_with_transport;
use forme_capabilities::ManagedMcpCatalogSource;

fn lost(f: &Fixture) -> (ReactiveHarness, Arc<Transport>, p::RunId) {
    let (h, transport) = harness(f);
    transport.lose_dispatch.store(true, Ordering::SeqCst);
    let run = submit(&h);
    h.resume(run.clone(), ResumeInput::Approval(grant(&h)))
        .unwrap();
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.cancels.load(Ordering::SeqCst), 0);
    (h, transport, run)
}
fn reopen(f: &Fixture, transport: Arc<Transport>) -> ReactiveHarness {
    let worker = backend_with_transport(f.sink.clone(), transport.clone());
    let (h, _) = harness_with_worker(f, worker, transport);
    h.recover_unknown_outcomes().unwrap();
    h.recover_owner_discoveries().unwrap();
    h
}
fn assert_once(h: &ReactiveHarness, transport: &Transport, run: &p::RunId) {
    let e = events(h, run);
    for kind in [
        p::EventKind::ActionStarted,
        p::EventKind::ActionCompleted,
        p::EventKind::McpDiscovered,
        p::EventKind::RunComplete,
    ] {
        assert_eq!(e.iter().filter(|v| v.kind == kind).count(), 1, "{kind:?}");
    }
    let gates = e
        .iter()
        .filter_map(|event| match &event.payload {
            p::EventPayload::CompetenceGateEvaluated(v) => Some(v.max_level),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(gates, vec![p::InterventionLevel::L0Observe; 2]);
    assert!(!e
        .iter()
        .any(|event| event.kind == p::EventKind::ModelCallStarted));
    let evidence = e
        .iter()
        .filter_map(|event| match &event.payload {
            p::EventPayload::CapabilityEvidenceRecorded(v) => Some(v.outcome.0.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(evidence, vec!["unknown"]);
    let catalog = crate::production_artifact_store::ProtectedMcpCatalogSource {
        store: h.store.clone(),
        owner: owner(),
        authority: h.owner_discovery.as_ref().unwrap().authority.clone(),
    }
    .read_catalog(&spec().server)
    .unwrap();
    assert_eq!(catalog.0, settings());
    assert_eq!(catalog.1.tools[0].name, "fixture.read");
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.cancels.load(Ordering::SeqCst), 0);
}

#[test]
fn lost_dispatch_response_recovers_expired_original_without_renewing_or_executing() {
    let f = Fixture::new();
    let (h, transport, run) = lost(&f);
    let original = transport.original().request;
    assert_eq!(
        read::<p::WorkerDispatchRequest>(&h, &run, "dispatch").unwrap(),
        Some(original.clone())
    );
    let delay = original.job.deadline.saturating_sub(now_ms()).max(0) as u64 + 5;
    std::thread::sleep(std::time::Duration::from_millis(delay));
    h.verify_managed_mcp(&owner(), run.clone()).unwrap();
    assert_once(&h, &transport, &run);
    assert_eq!(transport.original().request, original);
    let probes = transport.probes.load(Ordering::SeqCst);
    h.verify_managed_mcp(&owner(), run.clone()).unwrap();
    assert_eq!(transport.probes.load(Ordering::SeqCst), probes);
    assert_once(&h, &transport, &run);
}

#[test]
fn original_survives_restart_and_concurrent_verification_publishes_only_once() {
    let f = Fixture::new();
    let (h, transport, run) = lost(&f);
    let raw = serde_json::to_vec(&transport.original().request).unwrap();
    let db = std::fs::read(f.root.0.join("brain.db")).unwrap();
    assert!(!db.windows(raw.len()).any(|bytes| bytes == raw));
    let before = events(&h, &run);
    drop(h);
    let h = reopen(&f, transport.clone());
    assert_eq!(events(&h, &run), before);
    std::thread::scope(|scope| {
        let a = scope.spawn(|| h.verify_managed_mcp(&owner(), run.clone()));
        let b = scope.spawn(|| h.verify_managed_mcp(&owner(), run.clone()));
        a.join().unwrap().unwrap();
        b.join().unwrap().unwrap();
    });
    assert_once(&h, &transport, &run);
}

#[test]
fn missing_corrupt_or_noncompleted_original_keeps_history_unknown() {
    for change in 0..6 {
        let f = Fixture::new();
        let (h, transport, run) = lost(&f);
        let before = events(&h, &run);
        if change == 0 {
            transport.lose_result.store(true, Ordering::SeqCst);
        } else {
            transport.alter_original(|original| match change {
                1 => original.observation.nonce = p::Nonce("nonce:wrong".into()),
                2 => {
                    original.observation.state = p::WorkerState::Unknown;
                    original.observation.outcome = Some(p::WorkerOutcome::Unknown);
                }
                3 => {
                    original.manifest.binding.permit_digest =
                        p::canonical_digest(&"wrong permit").unwrap()
                }
                4 => original.bytes[0] ^= 1,
                _ => {
                    original.request.permit.issued_at += 1;
                    original.manifest.binding.permit_digest =
                        p::canonical_digest(&original.request.permit).unwrap();
                    original.manifest.manifest_digest =
                        original.manifest.expected_digest().unwrap();
                }
            });
        }
        // The last mutation changes the Host proof's identity; the immutable
        // Core request cannot be replaced with this different original.
        assert!(
            h.verify_managed_mcp(&owner(), run.clone()).is_err(),
            "change {change}"
        );
        assert_eq!(events(&h, &run), before);
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        assert_eq!(transport.cancels.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn changed_owner_configuration_authority_and_policy_block_before_probe() {
    for change in 0..6 {
        let f = Fixture::new();
        let (mut h, transport, run) = lost(&f);
        let before = events(&h, &run);
        let mut principal = owner();
        match change {
            0 => principal = p::VerifiedPrincipal("owner:foreign".into()),
            1 => h.owner_discovery.as_mut().unwrap().settings[0].enabled = false,
            2 => {
                h.owner_discovery.as_mut().unwrap().worker_digest =
                    p::canonical_digest(&"other worker").unwrap()
            }
            3 => h.composition_manifest = Some(p::canonical_digest(&"other composition").unwrap()),
            4 => h.config.policy_version.0 += 1,
            _ => h.governance.layers.push(PolicyLayer {
                schema_version: p::SchemaVersion(1),
                source: PolicyLayerSource::Managed,
                rules: vec![PolicyRule {
                    schema_version: p::SchemaVersion(1),
                    effect: p::PolicyDecision::Deny,
                    scope: p::Scope("*".into()),
                    matcher: ActionMatcher {
                        parameters: ArgMatcher::Any,
                        ..Default::default()
                    },
                }],
            }),
        }
        assert!(h.verify_managed_mcp(&principal, run.clone()).is_err());
        assert_eq!(events(&h, &run), before);
        assert_eq!(transport.probes.load(Ordering::SeqCst), 0);
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn retained_material_failure_can_be_read_again_after_restart_without_redispatch() {
    let f = Fixture::new();
    let (h, transport) = harness_with_consumer(&f, Arc::new(LostMaterial));
    let run = submit(&h);
    h.resume(run.clone(), ResumeInput::Approval(grant(&h)))
        .unwrap();
    assert!(h.verify_managed_mcp(&owner(), run.clone()).is_err());
    assert!(!events(&h, &run)
        .iter()
        .any(|e| e.kind == p::EventKind::ActionCompleted));
    drop(h);
    let h = reopen(&f, transport.clone());
    h.verify_managed_mcp(&owner(), run.clone()).unwrap();
    assert_once(&h, &transport, &run);
}

#[test]
fn restart_after_completion_does_not_finish_before_verified_publication() {
    let f = Fixture::new();
    let (h, transport, run) = lost(&f);
    let pending: PendingDiscovery = read(&h, &run, "pending").unwrap().unwrap();
    let mut plan = pending.plan.restore().unwrap();
    plan.approval_ref = Some(pending.approval.approval_id);
    let original = transport.original().request;
    let output = h
        .owner_discovery
        .as_ref()
        .unwrap()
        .backend
        .read_discovery_original(&plan, &original)
        .unwrap();
    let completion = output
        .into_iter()
        .find(|e| e.kind() == p::EventKind::ActionCompleted)
        .unwrap();
    h.append(
        run.clone(),
        None,
        untrusted_action_provenance(),
        stamp_external_payload(completion),
    )
    .unwrap();
    drop(h);
    let h = reopen(&f, transport.clone());
    assert!(!events(&h, &run)
        .iter()
        .any(|e| e.kind == p::EventKind::RunComplete));
    let probes = transport.probes.load(Ordering::SeqCst);
    h.publish_managed_mcp(&owner(), run.clone()).unwrap();
    assert_once(&h, &transport, &run);
    assert_eq!(transport.probes.load(Ordering::SeqCst), probes);
}

#[test]
fn contradictory_unknown_after_completion_is_never_treated_as_historical_pending() {
    let f = Fixture::new();
    let (h, transport, run) = lost(&f);
    h.verify_managed_mcp(&owner(), run.clone()).unwrap();
    h.append(
        run.clone(),
        None,
        untrusted_action_provenance(),
        p::EventPayload::ActionOutcomeUnknown(p::ActionOutcomeUnknownPayload {
            intent_id: transport.original().request.job.action_ref,
            probe_hint: p::ProbeHintRef(format!(
                "worker:{}",
                transport.original().request.job.job_id
            )),
            remote_lease: None,
        }),
    )
    .unwrap();
    let before = events(&h, &run);
    assert!(h.verify_managed_mcp(&owner(), run.clone()).is_err());
    assert!(h.publish_managed_mcp(&owner(), run.clone()).is_err());
    assert_eq!(events(&h, &run), before);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn lifecycle_deletion_prevents_reading_or_restoring_an_original() {
    let f = Fixture::new();
    let (h, transport, run) = lost(&f);
    let lifecycle = f.protection.lifecycle_service(f.store.clone());
    let request = f.deletion();
    let plan = lifecycle.plan(&request).unwrap();
    lifecycle.apply_plan(request, &plan.digest).unwrap();
    assert!(h.verify_managed_mcp(&owner(), run).is_err());
    assert_eq!(transport.probes.load(Ordering::SeqCst), 0);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn failure_to_retain_first_dispatch_prevents_started_and_transport() {
    let f = Fixture::new();
    let (h, transport) = harness(&f);
    let run = submit(&h);
    let approval = grant(&h);
    // The next protected-content key belongs to the original dispatch request.
    f.keys.fail_at.store(
        f.keys.allocations.load(Ordering::SeqCst) + 1,
        Ordering::SeqCst,
    );
    let _ = h.resume(run.clone(), ResumeInput::Approval(approval));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    assert!(!events(&h, &run).iter().any(|e| matches!(
        e.kind,
        p::EventKind::ActionStarted | p::EventKind::ActionCompleted
    )));
    assert!(read::<p::WorkerDispatchRequest>(&h, &run, "dispatch")
        .unwrap()
        .is_none());
    assert!(h.verify_managed_mcp(&owner(), run).is_err());
    assert_eq!(transport.probes.load(Ordering::SeqCst), 0);
}
