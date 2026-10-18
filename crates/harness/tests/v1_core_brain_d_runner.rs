//! The CLI's missing-credential path is tested in an isolated child environment.
#![forbid(unsafe_code)]

use forme_eval::{CoreBrainDArtifact, ProductEvidenceGrade, ProductVerdict};
use std::process::Command;

#[test]
fn runner_missing_model_credentials_emits_unavailable_exit2_and_refuses_overwrite() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("forme-c24-cli-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let out = root.join("new-evidence");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_forme-core-brain-d-golden"))
            .args(["--live", "--out"])
            .arg(&out)
            .env_remove("FORME_MODEL_PROFILES_JSON")
            .env_remove("FORME_MODEL_API_KEY")
            .env("FORME_MODEL_NAME", "unavailable-fixture")
            .env("FORME_MODEL_BASE_URL", "https://configured.invalid/v1")
            .output()
            .unwrap()
    };
    let result = run();
    assert_eq!(result.status.code(), Some(2));
    let raw = std::fs::read(out.join("product-golden.json")).unwrap();
    let artifact: CoreBrainDArtifact = serde_json::from_slice(&raw).unwrap();
    assert_eq!(artifact.grade, ProductEvidenceGrade::Unavailable);
    assert_eq!(
        artifact.verify().unwrap().verdict,
        ProductVerdict::Unverifiable
    );
    assert_eq!(artifact.tasks.len(), 6);
    assert!(artifact
        .tasks
        .iter()
        .all(|task| task.forme.is_empty() && task.direct_model.is_empty()));
    assert!(!String::from_utf8_lossy(&raw).contains("configured.invalid"));
    assert!(!String::from_utf8_lossy(&result.stderr).contains(&root.to_string_lossy().to_string()));
    assert_eq!(run().status.code(), Some(1));
    assert_eq!(std::fs::read(out.join("product-golden.json")).unwrap(), raw);
    // Only this exact test-created root is removed.
    let checked = root.canonicalize().unwrap();
    assert_eq!(
        checked.parent(),
        Some(std::env::temp_dir().canonicalize().unwrap().as_path())
    );
    std::fs::remove_dir_all(checked).unwrap();
}
