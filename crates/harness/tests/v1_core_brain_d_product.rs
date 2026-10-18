#[path = "support/core_brain.rs"]
mod support;
use forme_cognition::{CognitiveMapStore, MemoryGraph};
use forme_harness::*;
use forme_models::ModelSelector;
use forme_protocol as p;
use forme_store::EventStore;
use std::sync::atomic::Ordering;
use support::*;

fn request(key: &str, input: &str) -> p::RunRequest {
    p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::UserTurn,
        session: p::SessionRef(format!("session:{key}")),
        agent_profile: p::AgentProfileRef("agent:core-b".into()),
        input: p::RunInput(input.into()),
        budget: None,
        idempotency_key: Some(p::IdempotencyKey(key.into())),
    }
}
fn configure_fallback(f: &mut Fixture) {
    f.config.models[0].fallback_group = Some("bounded-d".into());
    let mut second = f.config.models[0].clone();
    second.id = "model:second".into();
    second.provider = "second-provider".into();
    second.model = "second-model".into();
    second.default = false;
    second.input_microunits_per_million += 10;
    f.config.models.push(second);
}
fn confirmation(plan: &DataOperationPlan) -> DataOperationConfirmation {
    DataOperationConfirmation {
        schema_version: p::SchemaVersion(1),
        plan_digest: plan.digest.clone(),
        nonce: plan.nonce.clone(),
        use_by: plan.expires_at,
        approved: true,
    }
}

#[test]
fn c19_actual_attempts_bind_distinct_providers_and_only_explicit_rejections_fallback() {
    let mut f = Fixture::new();
    configure_fallback(&mut f);
    f.statuses.lock().unwrap().push_back(503);
    *f.answer.lock().unwrap() = "completed-by-second".into();
    let runtime = f.build();
    let run = runtime
        .harness()
        .submit_run(request("d-fallback", "Answer with the fixture result"))
        .unwrap();
    assert_eq!(
        runtime
            .harness()
            .output_text(run.clone())
            .unwrap()
            .as_deref(),
        Some("completed-by-second")
    );
    let events = runtime
        .services()
        .store
        .read_run(run)
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    let profiles = events
        .iter()
        .filter_map(|e| {
            if let p::EventPayload::ModelCallStarted(v) = &e.payload {
                Some(v.model_profile.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        profiles,
        vec![
            p::ModelProfileRef("core-b-provider:core-b-model".into()),
            p::ModelProfileRef("second-provider:second-model".into())
        ]
    );
    let requests = f.requests.lock().unwrap();
    assert!(requests[0].contains("core-b-model"));
    assert!(requests[1].contains("second-model"));
    assert_eq!(f.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == p::EventKind::ModelCallFinished)
            .count(),
        2
    );
    assert_eq!(events.iter().filter(|e|matches!(&e.payload,p::EventPayload::ModelCallFinished(v) if v.finish_reason.0=="provider_error")).count(),1);
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == p::EventKind::FailureEvidenceRecorded)
            .count(),
        1
    );
    assert_eq!(
        runtime
            .dashboard(None, None, None)
            .unwrap()
            .metrics
            .model_fallback
            .numerator,
        1
    );
    let public = format!(
        "{:?}",
        events
            .iter()
            .filter(|e| e.kind == p::EventKind::FailureEvidenceRecorded)
            .collect::<Vec<_>>()
    );
    assert!(!public.contains("127.0.0.1"));
    assert!(!public.contains("core-b-local-fixture-key"));
}

#[test]
fn c19_unknown_timeout_and_incompatible_profiles_never_switch_models() {
    let mut f = Fixture::new();
    configure_fallback(&mut f);
    f.config.models[0].timeout_ms = 1; // curl's bounded timeout is one second.
    f.delay_ms.store(1300, Ordering::SeqCst);
    let runtime = f.build();
    let run = runtime
        .harness()
        .submit_run(request("d-unknown", "Return the result"))
        .unwrap();
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    let events = runtime
        .services()
        .store
        .read_run(run)
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == p::EventKind::ModelCallStarted)
            .count(),
        1
    );
    assert!(events
        .iter()
        .any(|e| e.kind == p::EventKind::FailureEvidenceRecorded));
    drop(runtime);
    f.delay_ms.store(0, Ordering::SeqCst);
    f.config.models[1].context_window = 1024;
    let runtime = f.build();
    let selected = runtime
        .services()
        .model_registry
        .select(&forme_models::ModelSelectionRequest {
            schema_version: p::SchemaVersion(1),
            task: None,
            minimum_context_window: 32768,
            require_tool_use: false,
            tool_schema_version: None,
            minimum_strength: forme_models::ModelStrength::Standard,
            risk: p::Risk::High,
            evidence: vec![],
        })
        .unwrap();
    assert!(selected.fallback_chain.is_empty());
}

#[test]
fn c20_dashboard_is_workspace_scoped_replayable_and_counts_actual_fallbacks() {
    let f = Fixture::new();
    let runtime = f.build();
    runtime
        .harness()
        .submit_run(request("d-dashboard", "Return a result"))
        .unwrap();
    let first = runtime.dashboard(None, None, None).unwrap();
    assert_eq!(first.metrics.model_fallback.numerator, 0);
    assert_eq!(first.metrics.model_attempts.denominator, 1);
    let other = runtime
        .dashboard(
            Some(p::WorkspaceRef("workspace:unrelated".into())),
            None,
            None,
        )
        .unwrap();
    assert_eq!(other.metrics.model_attempts.denominator, 0);
    let events = events(&runtime);
    let mut reversed = events.clone();
    reversed.reverse();
    assert_eq!(
        forme_eval::CognitiveDashboardProjection::from_events(
            &events,
            first.owner_digest.clone(),
            None,
            None,
            None
        )
        .unwrap(),
        forme_eval::CognitiveDashboardProjection::from_events(
            &reversed,
            first.owner_digest.clone(),
            None,
            None,
            None
        )
        .unwrap()
    );
    drop(runtime);
    let runtime = f.build();
    assert_eq!(first, runtime.dashboard(None, None, None).unwrap());
    assert!(runtime.dashboard(None, Some(20), Some(10)).is_err());
}

#[test]
fn c20_learning_scope_window_and_backlog_follow_the_original_proposal() {
    let f = Fixture::new();
    let runtime = f.build();
    let start = now();
    runtime
        .register_learning_goal(
            &f.owner(),
            p::IdempotencyKey("d-dashboard-goal".into()),
            p::GoalFrameRef("goal:d-learn".into()),
            knowledge("d-risk", p::Impact::High),
        )
        .unwrap();
    runtime.cognitive_tick_at(start).unwrap();
    let (proposal, _) = ask(&runtime);
    let answer_at = now();
    runtime
        .answer_learning(
            &f.owner(),
            p::IdempotencyKey("d-dashboard-reply".into()),
            p::LearningReplyRecord {
                schema_version: p::SchemaVersion(1),
                proposal,
                value: p::UserAttributeValueRef("answer:d-risk".into()),
            },
        )
        .unwrap();
    let snapshot = runtime
        .dashboard(Some(f.config.workspace.clone()), Some(answer_at), None)
        .unwrap();
    assert_eq!(snapshot.metrics.ask_to_learn_acceptance.numerator, 1);
    assert_eq!(snapshot.metrics.ask_to_learn_usefulness.unverifiable, 1);
    assert_eq!(snapshot.metrics.ask_to_learn_usefulness.numerator, 0);
    assert_eq!(snapshot.metrics.candidate_backlog, 1);
    assert!(!snapshot.stream_heads.is_empty());
    let unrelated = runtime
        .dashboard(
            Some(p::WorkspaceRef("workspace:d-other".into())),
            None,
            None,
        )
        .unwrap();
    assert_eq!(unrelated.metrics.candidate_backlog, 0);
    assert_eq!(unrelated.metrics.ask_to_learn_acceptance.denominator, 0);
    assert_eq!(unrelated.metrics.ask_to_learn_usefulness.unverifiable, 0);
}

#[test]
fn c21_owner_restore_binds_exact_backup_plan_and_one_shot_without_switching_authority() {
    let f = Fixture::new();
    let runtime = f.build();
    runtime
        .harness()
        .submit_run(request("d-backup", "Return a result"))
        .unwrap();
    runtime
        .create_data_backup(&f.owner(), "snapshot.db")
        .unwrap();
    assert!(runtime
        .verify_data_backup(&f.owner(), "snapshot.db")
        .unwrap()
        .is_valid());
    assert!(runtime
        .create_data_backup(&p::VerifiedPrincipal("other".into()), "bad.db")
        .is_err());
    assert!(runtime.create_data_backup(&f.owner(), "../bad.db").is_err());
    let plan = runtime
        .prepare_data_restore(&f.owner(), "snapshot.db".into(), "restored.db".into())
        .unwrap();
    let grant = confirmation(&plan);
    let mut forged = grant.clone();
    forged.nonce.0.push('x');
    assert!(runtime
        .apply_data_operation(&f.owner(), plan.clone(), forged)
        .is_err());
    let mut forged = plan.clone();
    if let DataPlannedOperation::Restore { target, .. } = &mut forged.operation {
        *target = "changed.db".into();
    }
    assert!(runtime
        .apply_data_operation(&f.owner(), forged, grant.clone())
        .is_err());
    let identity = runtime.services().authority.store_identity.clone();
    assert!(matches!(
        runtime
            .apply_data_operation(&f.owner(), plan.clone(), grant.clone())
            .unwrap(),
        DataOperationResult::Restored(_)
    ));
    assert_eq!(identity, runtime.services().authority.store_identity);
    assert!(runtime
        .apply_data_operation(&f.owner(), plan, grant)
        .is_err());
}

#[test]
fn c23_real_runtime_erases_derived_graph_and_cognition_evicts_cached_output_and_restarts() {
    use forme_cognition as c;
    let f = Fixture::new();
    let runtime = f.build();
    const PRIVATE: &str = "private-d-runtime-e79a309c";
    *f.answer.lock().unwrap() = PRIVATE.into();
    let run = runtime
        .harness()
        .submit_run(request("d-erase", PRIVATE))
        .unwrap();
    assert_eq!(
        runtime
            .harness()
            .output_text(run.clone())
            .unwrap()
            .as_deref(),
        Some(PRIVATE)
    );
    let source = runtime
        .services()
        .store
        .read_run(run.clone())
        .next()
        .unwrap()
        .unwrap()
        .event_id;
    let provenance = p::Provenance {
        source: p::Source::OwnerControl,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: Some(source.clone()),
    };
    let scope = p::Scope(f.config.workspace.0.clone());
    runtime
        .services()
        .memory
        .add_node(c::MemoryNode {
            schema_version: p::SchemaVersion(1),
            id: p::NodeId("node:d-erased".into()),
            kind: c::NodeKind::Fact,
            content_ref: p::ContentRef(PRIVATE.into()),
            tier: p::StabilityTier::Working,
            confidence: p::Confidence(0.4),
            scope: c::MemoryScope {
                schema_version: p::SchemaVersion(1),
                scope: scope.clone(),
                workspace: Some(f.config.workspace.clone()),
                slice: c::TimeSlice::LongTerm,
            },
            resting_activation: p::RestingActivation(0.0),
            recency: now(),
            provenance: provenance.clone(),
        })
        .unwrap();
    let map_scope = c::MapScope {
        schema_version: p::SchemaVersion(1),
        scope: scope.clone(),
    };
    let candidate = runtime
        .services()
        .cognition
        .propose_update(c::CognitiveMapUpdateProposal {
            schema_version: p::SchemaVersion(1),
            candidate_id: None,
            scope: map_scope.clone(),
            kind: c::MapUpdateKind::Frame,
            confidence: p::Confidence(0.3),
            evidence: vec![p::EvidenceRef(source.0.clone())],
            frame: Some(p::JudgmentFrameRef("frame:d-erased".into())),
            quality: None,
            blindspot: None,
            resource: None,
            reflection_inputs: vec![],
            statement: PRIVATE.into(),
            provenance: provenance.clone(),
        })
        .unwrap();
    let evidence = vec![c::GovernanceEvidence {
        schema_version: p::SchemaVersion(1),
        reference: p::EvidenceRef(source.0),
        observed_at: now(),
        verified_process: true,
    }];
    assert_eq!(
        runtime
            .services()
            .cognition
            .govern_and_apply(c::GovernanceCandidate {
                schema_version: p::SchemaVersion(1),
                candidate_id: candidate,
                direction: c::ChangeDirection::CautionIncreasing,
                confidence: p::Confidence(0.9),
                evidence: evidence.clone(),
                impact: p::Impact::Low,
                provenance,
                conflicts: vec![],
                owner_confirmed: true,
                target: c::StableCognition {
                    schema_version: p::SchemaVersion(1),
                    object: p::ObjectRef("frame:d-erased".into()),
                    scope: map_scope.clone(),
                    kind: c::StableCognitionKind::Frame,
                    statement: PRIVATE.into(),
                    tier: p::StabilityTier::Stable,
                    confidence: p::Confidence(0.9),
                    evidence,
                    last_reproduced_at: now(),
                    active: false
                }
            })
            .unwrap(),
        c::GovernanceDecision::Promote
    );
    assert!(!runtime
        .services()
        .cognition
        .read(map_scope.clone())
        .frames
        .is_empty());
    let plan = runtime
        .prepare_data_lifecycle(
            &f.owner(),
            scope,
            p::sha256_content_digest(run.0.as_bytes()),
            p::DataLifecycleOperation::CryptoShred,
            0,
        )
        .unwrap();
    let receipt = runtime
        .apply_data_operation(&f.owner(), plan.clone(), confirmation(&plan))
        .unwrap();
    assert!(matches!(receipt, DataOperationResult::Lifecycle(_)));
    assert!(runtime.harness().output_text(run.clone()).is_err());
    assert!(runtime
        .services()
        .memory
        .query(c::GraphQuery {
            content_contains: Some(PRIVATE.into()),
            ..c::GraphQuery::default()
        })
        .is_empty());
    assert!(runtime
        .services()
        .cognition
        .read(map_scope.clone())
        .frames
        .is_empty());
    assert!(runtime
        .services()
        .store
        .search(PRIVATE, 100)
        .unwrap()
        .is_empty());
    drop(runtime);
    let runtime = f.build();
    assert!(runtime
        .services()
        .cognition
        .read(map_scope)
        .frames
        .is_empty());
    assert!(runtime.services().store.read_run(run).next().is_none());
}

struct UnavailableDeletionKeys;
impl forme_store::ContentKeyResolver for UnavailableDeletionKeys {
    fn resolve(&self, _: &p::SchemaDigest) -> p::Result<Vec<u8>> {
        Err(p::Error("self-authored unavailable key fixture".into()))
    }
    fn destroy(&self, _: &p::SchemaDigest) -> p::Result<()> {
        Err(p::Error("self-authored interrupted key deletion".into()))
    }
}
#[test]
fn c23_pending_cleanup_fences_foreground_ticks_and_cached_reads_until_resume() {
    let f = Fixture::new();
    let runtime = f.build();
    let run = runtime
        .harness()
        .submit_run(request("d-pending-source", "Return a result"))
        .unwrap();
    let services = runtime.services();
    let scope = p::Scope(f.config.workspace.0.clone());
    let r = forme_store::DataLifecycleRequest {
        schema_version: p::V1_CLOSURE_SCHEMA_VERSION,
        owner: f.owner(),
        scope,
        subject_digest: p::sha256_content_digest(run.0.as_bytes()),
        operation: p::DataLifecycleOperation::CryptoShred,
        expected_version: 0,
        event_id: p::EventId("event:d-interrupted-control".into()),
        timestamp: now(),
        remote_disposition: p::RemoteDeletionDisposition::Unknown,
        evidence: vec![],
    };
    let interrupted = services
        .store
        .data_lifecycle_service(std::sync::Arc::new(UnavailableDeletionKeys));
    let plan = interrupted.plan(&r).unwrap();
    assert!(interrupted.apply_plan(r.clone(), &plan.digest).is_err());
    let calls = f.calls.load(Ordering::SeqCst);
    let event_count = events(&runtime).len();
    assert!(runtime
        .harness()
        .submit_run(request("d-pending-blocked", "Must not run"))
        .is_err());
    assert!(runtime.harness().output_text(run.clone()).is_err());
    assert!(runtime.cognitive_tick_at(now()).is_err());
    assert!(runtime.dashboard(None, None, None).is_err());
    assert!(runtime
        .create_data_backup(&f.owner(), "pending.db")
        .is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), calls);
    assert_eq!(events(&runtime).len(), event_count);
    let resumed = services
        .store
        .content_protection()
        .unwrap()
        .unwrap()
        .lifecycle_service(services.store.as_ref().clone());
    resumed.apply_plan(r, &plan.digest).unwrap();
    assert!(!services.store.has_pending_data_cleanup().unwrap());
    assert!(runtime.dashboard(None, None, None).is_ok());
    // Source was excluded at the first durable cleanup transaction.
    assert!(services.store.read_run(run).next().is_none());
}
