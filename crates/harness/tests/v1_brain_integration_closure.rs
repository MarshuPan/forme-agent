#[path = "support/core_brain.rs"]
mod support;
use forme_cognition as c;
use forme_context::{ContextBudget, ContextHydrationRequest, ContextHydrator};
use forme_harness::{AgentHarness, GatewayControl};
use forme_protocol as p;
use forme_store::EventStore;
use support::*;

fn request(key: &str) -> p::RunRequest {
    p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::UserTurn,
        session: p::SessionRef(format!("session:{key}")),
        agent_profile: p::AgentProfileRef("agent:core-b".into()),
        input: p::RunInput("Prepare a scoped project report".into()),
        budget: None,
        idempotency_key: Some(p::IdempotencyKey(key.into())),
    }
}

#[test]
fn production_observes_all_three_models_without_self_promoting_or_duplicate_recovery() {
    let f = Fixture::new();
    let runtime = f.build();
    let h = runtime.harness();
    let run = h.submit_run(request("three-models")).unwrap();
    assert_eq!(h.wait(run.clone()).unwrap().status, p::RunStatus::Complete);
    let services = runtime.services();
    let records = services.memory.candidate_records();
    assert!(records
        .iter()
        .any(|r| r.spec.id.0 == format!("run-user:{}", run.0)));
    assert!(records
        .iter()
        .any(|r| r.spec.target.0.starts_with("agent-self|")));
    assert!(records
        .iter()
        .any(|r| r.spec.target.0.starts_with("partnership|")));
    assert!(services.cognition.stable_objects().is_empty());
    let trace = services
        .store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    let before = events(&runtime).len();
    services
        .temporal_models
        .observe_terminal_run(&trace)
        .unwrap();
    assert_eq!(before, events(&runtime).len());
    // Merely acknowledging the observation cannot replace the evidence gate.
    let candidate = records
        .iter()
        .find(|r| r.spec.target.0.starts_with("agent-self|"))
        .unwrap();
    assert!(GatewayControl::review_candidate(
        h.as_ref(),
        p::CandidateReviewCommand {
            schema_version: p::SchemaVersion(1),
            run: services.aggregates.memory_cognition.clone(),
            candidate: candidate.spec.id.clone(),
            expected_state: p::CandidateReviewState::Candidate,
            decision: p::CandidateReviewDecision::Promote,
            actor: p::Actor::Owner,
            evidence: candidate.spec.evidence_refs.clone(),
            retraction: None,
        }
    )
    .is_err());
    assert!(services.cognition.stable_objects().is_empty());
    let inputs = services
        .competence_inputs(
            &p::Scope(f.config.workspace.0.clone()),
            Some(&p::CapabilityRef("owner-dialogue".into())),
        )
        .unwrap();
    assert!(
        inputs.capability_evidence.is_empty(),
        "a verified final string is not verified ability"
    );
    drop(services);
    drop(h);
    drop(runtime);
    let runtime = f.build();
    assert_eq!(runtime.services().memory.candidate_records(), records);
}

#[test]
fn scoped_stable_statements_reach_the_model_and_retraction_removes_them() {
    use c::{CognitiveMapStore, EvolutionGovernor};
    let f = Fixture::new();
    let runtime = f.build();
    let services = runtime.services();
    let scope = p::Scope(f.config.workspace.0.clone());
    let candidate = services
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
            evidence: vec![p::EvidenceRef("external-verified-process".into())],
            frame: None,
            quality: None,
            blindspot: Some(p::BlindSpotModelRef("blindspot:assumption".into())),
            resource: None,
            reflection_inputs: vec![],
            statement: "Verify units before comparing project measurements".into(),
            provenance: p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            },
        })
        .unwrap();
    let evidence = vec![c::GovernanceEvidence {
        schema_version: p::SchemaVersion(1),
        reference: p::EvidenceRef("external-verified-process".into()),
        observed_at: now(),
        verified_process: true,
    }];
    services
        .cognition
        .govern_and_apply(c::GovernanceCandidate {
            schema_version: p::SchemaVersion(1),
            candidate_id: candidate,
            direction: c::ChangeDirection::CautionIncreasing,
            confidence: p::Confidence(0.5),
            evidence: evidence.clone(),
            impact: p::Impact::Low,
            provenance: p::Provenance {
                source: p::Source::OwnerControl,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
            conflicts: vec![],
            owner_confirmed: true,
            target: c::StableCognition {
                schema_version: p::SchemaVersion(1),
                object: p::ObjectRef("blindspot:assumption".into()),
                scope: c::MapScope {
                    schema_version: p::SchemaVersion(1),
                    scope: scope.clone(),
                },
                kind: c::StableCognitionKind::BlindSpot,
                statement: "Verify units before comparing project measurements".into(),
                tier: p::StabilityTier::Stable,
                confidence: p::Confidence(0.5),
                evidence,
                last_reproduced_at: now(),
                active: false,
            },
        })
        .unwrap();
    let hydrate = || {
        services
            .context_hydrator
            .hydrate(&ContextHydrationRequest {
                schema_version: p::SchemaVersion(1),
                run: p::RunId("new-context".into()),
                session: p::SessionId("s".into()),
                workspace: f.config.workspace.clone(),
                scope: scope.clone(),
                brain_call: true,
                selected_skills: vec![],
                model: p::ModelProfileRef("model:core-b".into()),
                budget: ContextBudget {
                    schema_version: p::SchemaVersion(1),
                    max_tokens: 32768,
                    reserve: 1024,
                },
            })
            .unwrap()
    };
    assert!(format!("{:?}", hydrate()).contains("Verify units before comparing"));
    let h = runtime.harness();
    let run = h.submit_run(request("stable-context")).unwrap();
    h.wait(run).unwrap();
    assert!(f
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|r| r.contains("Verify units before comparing")));
    services.cognition.on_retraction(c::RetractionEvent {
        schema_version: p::SchemaVersion(1),
        target_object: p::ObjectRef("blindspot:assumption".into()),
        evidence_lineage: p::LineageRef("owner-correction".into()),
        provenance: p::Provenance {
            source: p::Source::OwnerControl,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        },
    });
    assert!(!format!("{:?}", hydrate()).contains("Verify units before comparing"));
    assert!(services
        .competence_inputs(&scope, None)
        .unwrap()
        .map_confidence
        .is_none());
}
