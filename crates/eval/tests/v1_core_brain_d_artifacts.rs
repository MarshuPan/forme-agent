use forme_eval::{
    CoreBrainDArtifact, ProductAttempt, ProductEvidenceGrade, ProductFileEvidence,
    ProductGoldenCase, ProductPredicate, ProductRestartEvidence, ProductVerdict,
};
use forme_protocol as p;

fn event(seq: u64, payload: p::EventPayload) -> p::Event {
    let mut e = p::Event::new(
        p::EventId(format!("event:d:{seq}")),
        p::RunId("run:d".into()),
        None,
        payload,
        p::SchemaVersion(1),
        seq as i64,
        p::Provenance {
            source: p::Source::Internal,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::VerifiedProcess,
            caused_by: None,
        },
    );
    e.stream_seq = seq;
    e
}
fn attempt() -> ProductAttempt {
    let profile = p::ModelProfileRef("fixture:d".into());
    let usage = p::ModelUsage {
        input_tokens: 12,
        output_tokens: 1,
    };
    let events = vec![
        event(
            1,
            p::EventPayload::SessionBound(p::SessionBoundPayload {
                policy_profile: p::PolicyProfileRef("policy:d".into()),
                model_profile: profile.clone(),
                toolset_ref: p::ToolsetRef("tools:d".into()),
                workspace: p::WorkspaceRef("workspace:d".into()),
                effect_mode: Some(p::EffectMode::LiveGoverned),
                evolution_snapshot: None,
                federation_snapshot: None,
                composition_manifest: Some(p::sha256_content_digest(b"composition")),
            }),
        ),
        event(
            2,
            p::EventPayload::ModelCallStarted(p::ModelCallStartedPayload {
                call_id: p::ModelCallId("call:d".into()),
                model_profile: profile.clone(),
            }),
        ),
        event(
            3,
            p::EventPayload::ModelCallFinished(p::ModelCallFinishedPayload {
                call_id: p::ModelCallId("call:d".into()),
                model_profile: profile,
                usage,
                finish_reason: p::FinishReason("stop".into()),
            }),
        ),
        event(
            4,
            p::EventPayload::RunComplete(p::RunCompletePayload {
                stop_reason: p::StopReason("completed".into()),
                result_ref: None,
            }),
        ),
    ];
    let mut a = ProductAttempt {
        output: Some("47".into()),
        latency_ms: 30,
        usage,
        ..ProductAttempt::empty()
    };
    a.capture_events(&events).unwrap();
    a
}
fn artifact() -> CoreBrainDArtifact {
    let mut a = CoreBrainDArtifact::unavailable("a".repeat(40)).unwrap();
    a.grade = ProductEvidenceGrade::Controlled;
    a.configuration_digest = Some(p::sha256_content_digest(b"controlled"));
    a.tasks[0].forme = vec![attempt(), attempt()];
    let mut baseline = ProductAttempt {
        output: Some("47".into()),
        ..ProductAttempt::empty()
    };
    baseline
        .model_profiles
        .insert(p::ModelProfileRef("fixture:d".into()));
    a.tasks[0].direct_model = vec![baseline.clone(), baseline];
    a.seal().unwrap();
    a
}
fn reseal_rejects(a: &mut CoreBrainDArtifact) {
    a.seal().unwrap();
    assert!(a.verify().is_err());
}

#[test]
fn c24_closed_six_cases_and_unavailable_never_claim_live_success() {
    let a = CoreBrainDArtifact::unavailable("a".repeat(40)).unwrap();
    assert_eq!(
        a.tasks.iter().map(|t| t.case).collect::<Vec<_>>(),
        ProductGoldenCase::ALL
    );
    assert_eq!(a.verify().unwrap().verdict, ProductVerdict::Unverifiable);
    let a = artifact();
    let scores = a.verify().unwrap();
    assert_eq!(scores.verdict, ProductVerdict::Unverifiable);
    assert_eq!(scores.tasks[0].quality_passes, 2);
    assert_eq!(scores.tasks[0].workflow_passes, 2);
    assert_eq!(scores.tasks[0].cost_microunits, None);
    let mut missing = a.clone();
    missing.tasks.pop();
    reseal_rejects(&mut missing);
    let mut changed = a.clone();
    changed.tasks[0].contract[0].predicate = ProductPredicate::Exact("wrong".into());
    reseal_rejects(&mut changed);
    let mut false_grade = a;
    false_grade.grade = ProductEvidenceGrade::Unavailable;
    reseal_rejects(&mut false_grade);
}

#[test]
fn c24_full_stream_provider_usage_and_workflow_evidence_cannot_be_replaced_by_text() {
    let original = artifact();
    let mut a = original.clone();
    a.tasks[0].forme[0].witnesses.remove(1);
    reseal_rejects(&mut a);
    let mut a = original.clone();
    a.tasks[0].forme[0].events.remove(1);
    reseal_rejects(&mut a);
    let mut a = original.clone();
    a.tasks[0].forme[0]
        .model_profiles
        .insert(p::ModelProfileRef("not-called".into()));
    reseal_rejects(&mut a);
    let mut a = original.clone();
    a.tasks[0].forme[0].usage.input_tokens = 0;
    reseal_rejects(&mut a);
    let mut a = original.clone();
    a.tasks[0].forme[0].events.clear();
    a.tasks[0].forme[0].witnesses.clear();
    a.tasks[0].forme[0].stream_heads.clear();
    reseal_rejects(&mut a);
    let mut a = original.clone();
    a.tasks[0].forme[0].restart = Some(ProductRestartEvidence {
        schema_version: p::SchemaVersion(1),
        owner_before: p::sha256_content_digest(b"owner"),
        owner_after: p::sha256_content_digest(b"wrong-owner"),
        state_before: p::sha256_content_digest(b"state"),
        state_after: p::sha256_content_digest(b"state"),
    });
    reseal_rejects(&mut a);
    let mut a = original.clone();
    a.tasks[0].forme[0].files.push(ProductFileEvidence {
        schema_version: p::SchemaVersion(1),
        relative_path: "result.txt".into(),
        bytes: b"47\n".to_vec(),
        digest: p::sha256_content_digest(b"wrong"),
    });
    reseal_rejects(&mut a);
    // Honest arithmetic output proves no multi-agent/project workflow.
    let mut a = original;
    let simple = a.tasks[0].forme.clone();
    a.tasks[3].forme = simple;
    a.seal().unwrap();
    assert_eq!(a.verify().unwrap().verdict, ProductVerdict::Fail);
}

#[test]
fn c24_resealed_private_data_and_unknown_schema_are_rejected() {
    for marker in [
        "Bearer not-a-real-key",
        "C:\\Users\\fixture\\private",
        "https://private.invalid/v1",
    ] {
        let mut a = artifact();
        a.tasks[0].forme[0].output = Some(marker.into());
        reseal_rejects(&mut a);
    }
    let mut value = serde_json::to_value(artifact()).unwrap();
    value["unapproved-field"] = serde_json::json!(true);
    assert!(serde_json::from_value::<CoreBrainDArtifact>(value).is_err());
    let mut a = artifact();
    a.tasks[0].forme[0].schema_version = p::SchemaVersion(99);
    reseal_rejects(&mut a);
}

#[test]
fn c24_offline_verifier_requires_live_pass_and_rejects_unknown_fields() {
    let path = std::env::temp_dir().join(format!("forme-d-verify-{}.json", std::process::id()));
    std::fs::write(&path, serde_json::to_vec(&artifact()).unwrap()).unwrap();
    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_forme-core-brain-d-artifact-verify"))
            .arg(&path)
            .arg("--require-pass")
            .output()
            .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let mut value = serde_json::to_value(artifact()).unwrap();
    value["unapproved-field"] = serde_json::json!(true);
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        !std::process::Command::new(env!("CARGO_BIN_EXE_forme-core-brain-d-artifact-verify"))
            .arg(&path)
            .output()
            .unwrap()
            .status
            .success()
    );
    std::fs::remove_file(path).unwrap();
}
