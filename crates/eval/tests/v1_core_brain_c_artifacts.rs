use forme_eval::{CoreBrainCArtifact, CoreBrainCCase};
use forme_protocol as p;
use std::path::PathBuf;

const FILES: [(&str, CoreBrainCCase); 6] = [
    ("patterns.json", CoreBrainCCase::Patterns),
    ("conflict.json", CoreBrainCCase::Conflict),
    ("recovery.json", CoreBrainCCase::Recovery),
    ("isolation.json", CoreBrainCCase::Isolation),
    ("supervision.json", CoreBrainCCase::Supervision),
    ("learning.json", CoreBrainCCase::Learning),
];
fn read(name: &str) -> CoreBrainCArtifact {
    let root = std::env::var_os("FORME_CORE_C_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/acceptance/core-brain-c-artifacts")
        });
    serde_json::from_slice(&std::fs::read(root.join(name)).unwrap()).unwrap()
}
fn reseal(a: &mut CoreBrainCArtifact) {
    a.digest = p::SchemaDigest(String::new());
    a.digest = p::canonical_digest(a).unwrap();
}
#[test]
fn c13_c18_six_portable_cases_replay_the_online_ledger_and_keep_actual_profiles() {
    for (name, case) in FILES {
        let artifact = read(name);
        assert_eq!(artifact.case, case);
        let ledger = artifact
            .validate()
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            p::canonical_digest(&ledger).unwrap(),
            artifact.online_ledger_digest
        );
        assert!(!artifact.model_bindings.is_empty());
    }
}
#[test]
fn c16_portable_witness_gaps_missing_payloads_digest_and_false_budget_are_rejected() {
    let original = read("learning.json");
    let mut changed = original.clone();
    changed.witnesses.remove(1);
    reseal(&mut changed);
    assert!(changed.validate().is_err());
    let mut changed = original.clone();
    changed
        .witnesses
        .iter_mut()
        .find(|w| w.kind == p::EventKind::ModelCallStarted)
        .unwrap()
        .event = None;
    reseal(&mut changed);
    assert!(changed.validate().is_err());
    let mut changed = original.clone();
    changed.online_ledger_digest = p::sha256_content_digest(b"wrong projection");
    reseal(&mut changed);
    assert!(changed.validate().is_err());
    let mut changed = original;
    let witness=changed.witnesses.iter_mut().find(|w|matches!(&w.event,Some(e) if matches!(&e.payload,
        p::EventPayload::DecisionTraceRecorded(t) if t.orchestration.as_ref().is_some_and(|f|matches!(f.operation,p::OrchestrationOperation::NodeFinished{..}))))).unwrap();
    let e = witness.event.as_mut().unwrap();
    if let p::EventPayload::DecisionTraceRecorded(t) = &mut e.payload {
        let f = t.orchestration.as_mut().unwrap();
        if let p::OrchestrationOperation::NodeFinished { consumed, .. } = &mut f.operation {
            *consumed = 999;
        }
        *f = p::OrchestrationFact::seal(
            f.workspace.clone(),
            f.command.clone(),
            f.expected_revision,
            f.operation.clone(),
        )
        .unwrap();
    }
    witness.digest = p::canonical_digest(e).unwrap();
    reseal(&mut changed);
    assert!(changed.validate().is_err());
}
#[test]
fn c16_portable_resealed_secret_private_path_model_and_schema_tampering_fail_closed() {
    let original = read("isolation.json");
    for marker in [
        "Bearer fixture-not-a-real-key",
        "C:\\Users\\fixture\\private",
        "https://private.invalid/v1",
    ] {
        let mut changed = original.clone();
        let w = changed
            .witnesses
            .iter_mut()
            .find(|w| w.kind == p::EventKind::DecisionTraceRecorded)
            .unwrap();
        if let p::EventPayload::DecisionTraceRecorded(t) = &mut w.event.as_mut().unwrap().payload {
            t.rationale = p::Rationale(marker.into());
        }
        w.digest = p::canonical_digest(w.event.as_ref().unwrap()).unwrap();
        reseal(&mut changed);
        assert!(changed
            .validate()
            .unwrap_err()
            .0
            .contains("private path marker"));
    }
    let mut changed = original.clone();
    changed.schema_version = p::SchemaVersion(0);
    reseal(&mut changed);
    assert!(changed.validate().is_err());
    let mut changed = original.clone();
    changed.files[0].digest = p::sha256_content_digest(b"different-file");
    reseal(&mut changed);
    assert!(changed.validate().is_err());
    let mut changed = original;
    let w = changed
        .witnesses
        .iter_mut()
        .find(|w| w.kind == p::EventKind::ModelCallStarted)
        .unwrap();
    if let p::EventPayload::ModelCallStarted(c) = &mut w.event.as_mut().unwrap().payload {
        c.model_profile = p::ModelProfileRef("model:unregistered".into());
    }
    w.digest = p::canonical_digest(w.event.as_ref().unwrap()).unwrap();
    reseal(&mut changed);
    assert!(changed.validate().is_err());
}
#[test]
fn c16_portable_cli_verifies_a_real_file_and_rejects_unknown_fields() {
    let artifact = read("recovery.json");
    let path = std::env::temp_dir().join(format!(
        "forme-c-verify-{}-{}.json",
        std::process::id(),
        p::canonical_digest(&artifact).unwrap().0.replace(':', "-")
    ));
    std::fs::write(&path, serde_json::to_vec(&artifact).unwrap()).unwrap();
    let output =
        std::process::Command::new(env!("CARGO_BIN_EXE_forme-core-brain-c-artifact-verify"))
            .arg(&path)
            .output()
            .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut value = serde_json::to_value(artifact).unwrap();
    value["unknown-control-field"] = serde_json::json!(true);
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        !std::process::Command::new(env!("CARGO_BIN_EXE_forme-core-brain-c-artifact-verify"))
            .arg(&path)
            .output()
            .unwrap()
            .status
            .success()
    );
    std::fs::remove_file(path).unwrap();
}
