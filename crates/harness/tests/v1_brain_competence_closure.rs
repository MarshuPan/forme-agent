#[path = "support/core_brain.rs"]
mod support;
use forme_cognition as c;
use forme_cognition::{AgentSelfModel, CognitiveMapStore, CompetenceGate, EvolutionGovernor};
use forme_harness::{AgentHarness, GatewayControl, ProductionRuntime, ResumeInput};
use forme_protocol as p;
use support::*;

fn fixture() -> Fixture {
    let mut f = Fixture::new();
    f.config.capabilities.file.enabled = true;
    f.config.capabilities.file.roots = vec![f
        .config
        .store_path
        .parent()
        .unwrap()
        .to_string_lossy()
        .into_owned()];
    f.config.models[0].tool_use = true;
    f.config.models[0].tool_schema_versions = vec![1];
    f
}
fn request(key: &str) -> p::RunRequest {
    p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::UserTurn,
        session: p::SessionRef(key.into()),
        agent_profile: p::AgentProfileRef("agent:core-b".into()),
        input: p::RunInput("Perform the approved scoped file action".into()),
        budget: None,
        idempotency_key: Some(p::IdempotencyKey(key.into())),
    }
}
fn call(path: &str, operation: p::FileOperation) -> serde_json::Value {
    let parameters = p::ActionParameters::File {
        operation,
        path: path.into(),
        content: (operation == p::FileOperation::Write).then(|| b"verified content".to_vec()),
    };
    serde_json::json!({"choices":[{"message":{"tool_calls":[{"id":"owned-file-call","type":"function",
        "function":{"name":forme_models::ToolDefinition::catalog_alias("capability:file").unwrap(),
        "arguments":serde_json::to_string(&parameters).unwrap()}}]},"finish_reason":"tool_calls"}],
        "usage":{"prompt_tokens":20,"completion_tokens":4}})
}
fn execute(
    runtime: &ProductionRuntime,
    f: &Fixture,
    key: &str,
    operation: p::FileOperation,
) -> p::RunId {
    let path = f
        .config
        .store_path
        .parent()
        .unwrap()
        .join(format!("{key}.txt"));
    f.responses
        .lock()
        .unwrap()
        .push_back(call(&path.to_string_lossy(), operation));
    let h = runtime.harness();
    let run = h.submit_run(request(key)).unwrap();
    if let Some(a) = h
        .pending_approvals(p::SessionId(key.into()))
        .unwrap()
        .into_iter()
        .next()
    {
        h.resume(
            run.clone(),
            ResumeInput::Approval(forme_harness::ApprovalGrant {
                schema_version: p::SchemaVersion(1),
                approval_id: a.approval_id.clone(),
                outcome: p::ApprovalOutcome::Granted,
                granted_scope: forme_approval::GrantScope::OneShot,
                approver: f.owner(),
                bound_plan_digest: a.plan_digest,
                normalized_action_digest: a.normalized_action_digest,
                affected_resources: a.affected_resources,
                policy_version: a.policy_version,
                tool_schema_version: a.tool_schema_version,
                nonce: p::Nonce(format!("nonce:{}", a.approval_id.0)),
                use_by: a.expires_at,
            }),
        )
        .unwrap();
    }
    h.wait(run.clone()).unwrap();
    run
}
fn owner_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::OwnerControl,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    }
}
fn establish(runtime: &ProductionRuntime, scope: &p::Scope, evidence: Vec<c::TemporalEvidence>) {
    let services = runtime.services();
    let temporal = &services.temporal_models;
    let id = AgentSelfModel::observe(
        temporal.as_ref(),
        c::SelfObservation {
            schema_version: p::SchemaVersion(1),
            capability: p::CapabilityRef("capability:file".into()),
            reliability: p::Reliability("verified".into()),
            gap: None,
            evidence: evidence.clone(),
            confidence: p::Confidence(0.9),
            scope: scope.clone(),
            time_scale: c::TimeScale::Repeated,
            provenance: owner_provenance(),
            initial_profile: false,
        },
    )
    .unwrap();
    assert_eq!(
        temporal.review_candidate(id, true).unwrap(),
        c::GovernanceDecision::Promote
    );
    let id = temporal
        .propose_trust(c::TrustObservation {
            schema_version: p::SchemaVersion(1),
            scope: scope.clone(),
            trust_level: c::TrustLevel::Established,
            delegation_state: c::DelegationState::Bounded,
            evidence: evidence.clone(),
            time_scale: c::TimeScale::Repeated,
            provenance: owner_provenance(),
            evidence_priority: c::EvidencePriority::Process,
        })
        .unwrap();
    assert_eq!(
        temporal.review_candidate(id, true).unwrap(),
        c::GovernanceDecision::Promote
    );
    let id = services
        .cognition
        .propose_update(c::CognitiveMapUpdateProposal {
            schema_version: p::SchemaVersion(1),
            candidate_id: None,
            scope: c::MapScope {
                schema_version: p::SchemaVersion(1),
                scope: scope.clone(),
            },
            kind: c::MapUpdateKind::BlindSpot,
            confidence: p::Confidence(0.5),
            evidence: evidence.iter().map(|e| e.reference.clone()).collect(),
            frame: None,
            quality: None,
            blindspot: Some(p::BlindSpotModelRef("map:file-evidence".into())),
            resource: None,
            reflection_inputs: vec![],
            statement: "Verify actual file contents before considering a write complete".into(),
            provenance: owner_provenance(),
        })
        .unwrap();
    let verified = evidence
        .iter()
        .map(|e| c::GovernanceEvidence {
            schema_version: p::SchemaVersion(1),
            reference: e.reference.clone(),
            observed_at: e.observed_at,
            verified_process: true,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        services
            .cognition
            .govern_and_apply(c::GovernanceCandidate {
                schema_version: p::SchemaVersion(1),
                candidate_id: id,
                direction: c::ChangeDirection::CautionIncreasing,
                confidence: p::Confidence(0.9),
                evidence: verified.clone(),
                impact: p::Impact::Low,
                provenance: owner_provenance(),
                conflicts: vec![],
                owner_confirmed: true,
                target: c::StableCognition {
                    schema_version: p::SchemaVersion(1),
                    object: p::ObjectRef("map:file-evidence".into()),
                    scope: c::MapScope {
                        schema_version: p::SchemaVersion(1),
                        scope: scope.clone()
                    },
                    kind: c::StableCognitionKind::BlindSpot,
                    statement: "Verify actual file contents before considering a write complete"
                        .into(),
                    tier: p::StabilityTier::Stable,
                    confidence: p::Confidence(0.9),
                    evidence: verified,
                    last_reproduced_at: now(),
                    active: false,
                },
            })
            .unwrap(),
        c::GovernanceDecision::Promote
    );
}

#[test]
fn competence_reads_real_verified_results_deduplicates_and_downgrades_on_failure_or_retraction() {
    let f = fixture();
    let runtime = f.build();
    let scope = p::Scope(f.config.workspace.0.clone());
    let capability = p::CapabilityRef("capability:file".into());
    let read = || {
        runtime
            .services()
            .competence_inputs(&scope, Some(&capability))
            .unwrap()
    };
    let gate = c::EvidenceCompetenceGate::default();
    assert_eq!(
        gate.ceiling(scope.clone(), p::Risk::Low, &read()),
        c::InterventionLevel::L1Suggest
    );
    let first = execute(&runtime, &f, "verified-first", p::FileOperation::Write);
    let first_inputs = read();
    assert_eq!(first_inputs.capability_evidence.len(), 1);
    assert_eq!(first_inputs.verification.len(), 1);
    assert_eq!(
        first_inputs.capability_evidence[0].reference.0,
        first_inputs.verification[0].reference.0
    );
    let second = execute(&runtime, &f, "verified-second", p::FileOperation::Write);
    let evidence = events(&runtime)
        .into_iter()
        .filter(|e| e.run_id == first || e.run_id == second)
        .filter(|e| {
            matches!(&e.payload, p::EventPayload::CapabilityEvidenceRecorded(p)
            if p.reliability.0 == "verified" && p.outcome.0 == "pass")
        })
        .map(|e| c::TemporalEvidence {
            schema_version: p::SchemaVersion(1),
            reference: p::EvidenceRef(e.event_id.0),
            observed_at: e.ts_unix_ms,
            verified_process: true,
        })
        .collect::<Vec<_>>();
    assert_eq!(evidence.len(), 2);
    establish(&runtime, &scope, evidence);
    let confident = read();
    assert_eq!(confident.capability_evidence.len(), 2);
    assert_eq!(
        gate.ceiling(scope.clone(), p::Risk::Low, &confident),
        c::InterventionLevel::L4Autonomous
    );
    let mut single = confident.clone();
    single.capability_evidence.truncate(1);
    single.verification.truncate(1);
    assert_eq!(
        gate.ceiling(scope.clone(), p::Risk::Low, &single),
        c::InterventionLevel::L3ActWithApproval,
        "one verified result appearing through two views is still one result"
    );
    let other = runtime
        .services()
        .competence_inputs(&p::Scope("workspace:other".into()), Some(&capability))
        .unwrap();
    assert!(other.self_model.is_none() && other.map_confidence.is_none() && other.trust.is_none());
    assert!(other.capability_evidence.is_empty());
    execute(&runtime, &f, "missing-file", p::FileOperation::Read);
    assert!(!read().failure.is_empty());
    assert!(
        gate.ceiling(scope.clone(), p::Risk::Low, &read()) < c::InterventionLevel::L4Autonomous
    );
    runtime
        .services()
        .cognition
        .on_retraction(c::RetractionEvent {
            schema_version: p::SchemaVersion(1),
            target_object: c::agent_self_object(&capability, &scope),
            evidence_lineage: p::LineageRef("owner:unreliable-now".into()),
            provenance: owner_provenance(),
        });
    assert!(read().self_model.is_none());
    assert!(gate.ceiling(scope.clone(), p::Risk::Low, &read()) <= c::InterventionLevel::L1Suggest);
    let expected = read();
    drop(runtime);
    let runtime = f.build();
    assert_eq!(
        runtime
            .services()
            .competence_inputs(&scope, Some(&capability))
            .unwrap(),
        expected
    );
    // A map candidate in the shared aggregate may cite a verified fact in
    // an execution stream. Review must resolve that fact, not project All
    // or accept caller-supplied evidence metadata.
    let failure = events(&runtime)
        .into_iter()
        .find(|e| matches!(e.payload, p::EventPayload::FailureEvidenceRecorded(_)))
        .unwrap();
    let candidate = p::CandidateId("cross-stream-caution".into());
    let caution = c::CognitiveMapUpdateProposal {
        schema_version: p::SchemaVersion(1),
        candidate_id: Some(candidate.clone()),
        scope: c::MapScope {
            schema_version: p::SchemaVersion(1),
            scope: scope.clone(),
        },
        kind: c::MapUpdateKind::BlindSpot,
        confidence: p::Confidence(0.5),
        evidence: vec![p::EvidenceRef(failure.event_id.0.clone())],
        frame: None,
        quality: None,
        blindspot: Some(p::BlindSpotModelRef("caution:verified-failure".into())),
        resource: None,
        reflection_inputs: vec![],
        statement: "Check file existence before planning a read".into(),
        provenance: owner_provenance(),
    };
    runtime
        .services()
        .cognition
        .propose_update(caution.clone())
        .unwrap();
    runtime
        .harness()
        .review_candidate(p::CandidateReviewCommand {
            schema_version: p::SchemaVersion(1),
            run: runtime.services().aggregates.memory_cognition.clone(),
            candidate,
            expected_state: p::CandidateReviewState::Candidate,
            decision: p::CandidateReviewDecision::Promote,
            actor: p::Actor::Owner,
            evidence: vec![p::EvidenceRef(failure.event_id.0.clone())],
            retraction: None,
        })
        .unwrap();
    assert!(runtime
        .services()
        .cognition
        .stable_objects()
        .iter()
        .any(|s| s.object.0 == "caution:verified-failure"
            && s.active
            && s.evidence[0].verified_process));
    let mut foreign = caution;
    let foreign_id = p::CandidateId("foreign-evidence-caution".into());
    foreign.candidate_id = Some(foreign_id.clone());
    foreign.scope.scope = p::Scope("workspace:unrelated".into());
    foreign.blindspot = Some(p::BlindSpotModelRef("caution:foreign-evidence".into()));
    runtime
        .services()
        .cognition
        .propose_update(foreign)
        .unwrap();
    assert!(runtime
        .harness()
        .review_candidate(p::CandidateReviewCommand {
            schema_version: p::SchemaVersion(1),
            run: runtime.services().aggregates.memory_cognition.clone(),
            candidate: foreign_id,
            expected_state: p::CandidateReviewState::Candidate,
            decision: p::CandidateReviewDecision::Promote,
            actor: p::Actor::Owner,
            evidence: vec![p::EvidenceRef(failure.event_id.0)],
            retraction: None,
        })
        .is_err());
    assert!(!runtime
        .services()
        .cognition
        .stable_objects()
        .iter()
        .any(|s| s.object.0 == "caution:foreign-evidence"));
}

#[test]
fn registering_an_independent_workspace_does_not_grant_tools_or_cross_scope_actions() {
    let f = fixture();
    let runtime = f.build();
    let workspace = p::WorkspaceRef("workspace:analysis-only".into());
    let change = runtime
        .prepare_workspace_charter(
            &f.owner(),
            p::WorkspaceCharterRecord {
                schema_version: p::SchemaVersion(1),
                workspace: workspace.clone(),
                version: 1,
                goals: vec![],
                constraints: vec![p::Constraint("max-nodes:8".into())],
                prohibitions: vec![],
                done_contract: None,
                review_cadence: None,
                actor: p::Actor::Owner,
                digest: p::SchemaDigest(String::new()),
            },
            0,
        )
        .unwrap();
    runtime
        .apply_workspace_charter(
            &f.owner(),
            p::IdempotencyKey("analysis-only".into()),
            change,
        )
        .unwrap();
    let run = runtime
        .submit_workspace_run(&f.owner(), &workspace, request("foreign-text"))
        .unwrap();
    assert_eq!(
        runtime.harness().wait(run).unwrap().status,
        p::RunStatus::Complete
    );
    let alias = forme_models::ToolDefinition::catalog_alias("capability:file").unwrap();
    assert!(!f.requests.lock().unwrap().last().unwrap().contains(&alias));
    let path = f
        .config
        .store_path
        .parent()
        .unwrap()
        .join("must-not-be-written.txt");
    f.responses
        .lock()
        .unwrap()
        .push_back(call(&path.to_string_lossy(), p::FileOperation::Write));
    let run = runtime
        .submit_workspace_run(&f.owner(), &workspace, request("foreign-invalid-tool"))
        .unwrap();
    assert!(matches!(
        runtime.harness().wait(run).unwrap().status,
        p::RunStatus::Complete
            | p::RunStatus::Aborted
            | p::RunStatus::Failed
            | p::RunStatus::Limited
    ));
    assert!(!path.exists());
    assert!(!events(&runtime)
        .iter()
        .any(|e| e.kind == p::EventKind::ActionStarted));
}
