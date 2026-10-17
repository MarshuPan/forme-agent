#[path = "support/core_brain.rs"]
mod support;
use forme_cognition as c;
use forme_cognition::MemoryGraph;
use forme_harness::*;
use forme_protocol as p;
use forme_store::EventStore;
use std::sync::atomic::Ordering;
use support::*;

fn populate(runtime: &ProductionRuntime) -> c::MemoryScope {
    let scope = c::MemoryScope {
        schema_version: p::SchemaVersion(1),
        slice: c::TimeSlice::LongTerm,
        workspace: Some(p::WorkspaceRef("workspace:core-b".into())),
        scope: p::Scope("workspace:core-b".into()),
    };
    for (id, kind, confidence) in [
        ("topic", c::NodeKind::Fact, 0.7),
        ("useful", c::NodeKind::Goal, 0.4),
        ("other", c::NodeKind::Fact, 0.7),
    ] {
        runtime
            .services()
            .memory
            .add_node(c::MemoryNode {
                schema_version: p::SchemaVersion(1),
                id: p::NodeId(id.into()),
                kind,
                content_ref: p::ContentRef(format!("reference:{id}")),
                tier: p::StabilityTier::Working,
                confidence: p::Confidence(confidence),
                scope: scope.clone(),
                resting_activation: p::RestingActivation(0.0),
                recency: now(),
                provenance: p::Provenance {
                    source: p::Source::Internal,
                    actor: p::Actor::System,
                    trust_tier: p::TrustTier::VerifiedProcess,
                    caused_by: None,
                },
            })
            .unwrap();
    }
    runtime
        .services()
        .memory
        .add_edge(c::MemoryEdge {
            schema_version: p::SchemaVersion(1),
            id: p::EdgeId("related".into()),
            from: p::NodeId("topic".into()),
            to: p::NodeId("useful".into()),
            kind: c::EdgeKind::Association,
            weight: p::Weight(0.7),
            provenance: p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            },
        })
        .unwrap();
    scope
}

#[test]
fn c10_formal_tick_uses_explicit_bounded_diffusion_and_restart_does_not_self_trigger() {
    let mut f = Fixture::new();
    f.config.cognition.relevance_mode = p::RelevanceMode::Diffusion;
    let runtime = f.build();
    populate(&runtime);
    let at = now();
    let CognitiveTickOutcome::Ran(report) = runtime.cognitive_tick_at(at).unwrap() else {
        panic!()
    };
    assert!(!report.failed());
    let first = events(&runtime);
    assert!(first
        .iter()
        .any(|event| event.kind == p::EventKind::ImpulseRaised));
    assert!(!first
        .iter()
        .any(|event| event.kind == p::EventKind::ActionStarted));
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    let count = first
        .iter()
        .filter(|e| e.kind == p::EventKind::ImpulseRaised)
        .count();
    drop(runtime);
    let runtime = f.build();
    runtime.cognitive_tick_at(at + 60_001).unwrap();
    assert_eq!(
        events(&runtime)
            .iter()
            .filter(|e| e.kind == p::EventKind::ImpulseRaised)
            .count(),
        count
    );
}

#[test]
fn c11_common_snapshot_provider_baseline_records_honest_metrics_and_negative_result() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = String::from("[\"useful\"]");
    let runtime = f.build();
    let scope = populate(&runtime);
    let case = RelevanceCase {
        schema_version: p::SchemaVersion(1),
        case_id: "relevance-negative-01".into(),
        cue: c::RecallCue {
            schema_version: p::SchemaVersion(1),
            scope,
            query: "Find useful context for a long term goal".into(),
        },
        seeds: vec![p::NodeId("topic".into())],
        shape: c::ActivationShape::Association,
        k: 1,
        graph_version: None,
        ground_truth: vec![p::NodeId("useful".into())],
        label_source: p::RelevanceLabelSource::IndependentFixture,
        evidence_level: p::RelevanceEvidenceLevel::ControlledHttp,
        at: now(),
    };
    let report = runtime.compare_relevance(&f.owner(), case.clone()).unwrap();
    report.validate().unwrap();
    assert_eq!(report.recommended, p::RelevanceMode::DirectModel);
    assert_eq!(report.direct_metrics.recall_bps, 10_000);
    assert_eq!(report.diffusion_metrics.recall_bps, 0);
    assert_eq!(report.model_profile.0, "core-b-provider:core-b-model");
    assert_eq!(report.direct_metrics.usage.input_tokens, 20);
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    let all = events(&runtime);
    assert!(all.iter().any(|e|matches!(&e.payload,p::EventPayload::ModelCallStarted(p) if p.call_id==report.call_id && p.model_profile==report.model_profile)));
    assert!(all.iter().any(|e|matches!(&e.payload,p::EventPayload::ModelCallFinished(p) if p.call_id==report.call_id && p.usage==report.direct_metrics.usage)));
    let mut tamper = report.clone();
    tamper.direct_metrics.hits = 0;
    assert!(tamper.validate().is_err());
    let mut tamper = report.clone();
    tamper.recommended = p::RelevanceMode::Diffusion;
    assert!(tamper.seal().is_err());
    if let Some(root) = std::env::var_os("FORME_CORE_B_ARTIFACT_DIR") {
        let root = std::path::PathBuf::from(root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("relevance-comparison.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
    *f.answer.lock().unwrap() = String::from("[\"outside\"]");
    assert!(runtime.compare_relevance(&f.owner(), case).is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), 2);
    assert!(!events(&runtime)
        .iter()
        .any(|e| e.kind == p::EventKind::ActionStarted));
}

#[test]
fn c11_formal_tick_can_use_direct_model_without_diffusion_or_hidden_profile() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = String::from("[\"useful\"]");
    let runtime = f.build();
    populate(&runtime);
    let CognitiveTickOutcome::Ran(report) = runtime.cognitive_tick_at(now()).unwrap() else {
        panic!()
    };
    assert!(!report.failed());
    assert!(report.phases[0].model_calls > 0);
    assert_eq!(
        u64::from(report.phases[0].model_calls),
        f.calls.load(Ordering::SeqCst)
    );
    assert!(events(&runtime)
        .iter()
        .filter_map(
            |e| if let p::EventPayload::ModelCallStarted(p) = &e.payload {
                Some(p.model_profile.clone())
            } else {
                None
            }
        )
        .all(|profile| profile.0 == "core-b-provider:core-b-model"));
    assert!(!runtime
        .services()
        .store
        .read_run(report.run.clone())
        .map(Result::unwrap)
        .any(|event| event.kind == p::EventKind::ActionStarted));
}
