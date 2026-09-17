#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use forme_eval::{M3CArtifactStore, M3CGoldenArtifactBody, PortableM3CGoldenArtifact};
use forme_protocol as p;

static NEXT: AtomicU64 = AtomicU64::new(1);

fn repository_artifact_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/acceptance/m3-c-artifacts")
}

fn repository_artifact() -> PortableM3CGoldenArtifact {
    let root = repository_artifact_root();
    let entries = fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", root.display()))
        .collect::<std::io::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        entries.len(),
        1,
        "repository M3-C artifact set is not closed"
    );
    serde_json::from_slice(&fs::read(entries[0].path()).unwrap()).unwrap()
}

fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "forme-m3-c-artifact-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn rejects_body(label: &str, body: &M3CGoldenArtifactBody) {
    let root = temp_root(label);
    let result = M3CArtifactStore::new(&root).and_then(|store| store.write(body));
    assert!(
        result.is_err(),
        "invalid M3-C artifact was accepted: {label}"
    );
    fs::remove_dir_all(root).unwrap();
}

fn copy_repository_artifact(destination: &Path) -> PathBuf {
    let source_root = repository_artifact_root();
    let source = fs::read_dir(source_root)
        .unwrap()
        .next()
        .expect("repository M3-C artifact is missing")
        .unwrap()
        .path();
    let destination_path = destination.join(source.file_name().unwrap());
    fs::copy(source, &destination_path).unwrap();
    destination_path
}

#[test]
fn s68_repository_golden_is_content_addressed_and_offline_verifiable() {
    let root = repository_artifact_root();
    let receipt = M3CArtifactStore::new(&root)
        .and_then(|store| store.verify_complete_set())
        .unwrap();
    assert!(receipt.digest.0.starts_with("fnv64:"));
    assert!(receipt.content_ref.0.starts_with("artifact:fnv64:"));
}

#[test]
fn s68_missing_phase_hidden_regression_and_final_v2_are_rejected() {
    let artifact = repository_artifact();

    let mut missing_phase = artifact.body.clone();
    missing_phase.control_events.remove(5);
    rejects_body("missing-phase", &missing_phase);

    let mut hidden_regression = artifact.body.clone();
    hidden_regression.regression_evaluation.verdict = p::EvaluationVerdict::Pass;
    rejects_body("hidden-regression", &hidden_regression);

    let mut final_v2 = artifact.body;
    final_v2.final_active.version = final_v2.candidate.proposed_version.clone();
    rejects_body("final-v2", &final_v2);
}

#[test]
fn s68_secret_and_private_path_are_rejected() {
    let artifact = repository_artifact();

    let mut secret = artifact.body.clone();
    secret.case_ref = p::EvaluationCaseRef("credential:raw-value".into());
    rejects_body("secret", &secret);

    let mut private_path = artifact.body;
    private_path.case_ref = p::EvaluationCaseRef("C:\\Users\\owner\\private".into());
    rejects_body("private-path", &private_path);
}

#[test]
fn s68_tamper_and_extra_artifact_are_rejected() {
    let tamper_root = temp_root("tamper");
    let path = copy_repository_artifact(&tamper_root);
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["body"]["external_mutation_count"] = serde_json::Value::from(4);
    fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    assert!(M3CArtifactStore::new(&tamper_root)
        .and_then(|store| store.verify_complete_set())
        .is_err());
    fs::remove_dir_all(tamper_root).unwrap();

    let extra_root = temp_root("extra");
    copy_repository_artifact(&extra_root);
    fs::write(extra_root.join("unexpected.txt"), b"unexpected").unwrap();
    assert!(M3CArtifactStore::new(&extra_root)
        .and_then(|store| store.verify_complete_set())
        .is_err());
    fs::remove_dir_all(extra_root).unwrap();
}
