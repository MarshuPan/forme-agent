use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use forme_eval::{
    LongHorizonArtifactBody, LongHorizonArtifactCheckpoint, LongHorizonArtifactDisposition,
    M3BArtifactStore,
};
use forme_protocol as p;

static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "forme-m3-b-artifact-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn checkpoint(
    index: u16,
    snapshot: &str,
    loop_version: &str,
    outward: bool,
    foreground_yield: bool,
) -> LongHorizonArtifactCheckpoint {
    let mut event_kinds = vec![
        p::EventKind::RunAccepted,
        p::EventKind::SessionBound,
        p::EventKind::GoalFramed,
        p::EventKind::OrchestrationRouteCreated,
        p::EventKind::SubagentSpawned,
        p::EventKind::SubagentResultReturned,
        p::EventKind::MemoryNodeAppended,
    ];
    if foreground_yield {
        event_kinds.extend([p::EventKind::RunWaiting, p::EventKind::RunResumed]);
    }
    event_kinds.extend([
        p::EventKind::VerificationStarted,
        p::EventKind::VerificationFinished,
        p::EventKind::RunComplete,
    ]);
    let event_refs = event_kinds
        .iter()
        .enumerate()
        .map(|(position, _)| p::EventId(format!("event:m3-b:{index}:{position}")))
        .collect();
    LongHorizonArtifactCheckpoint {
        schema_version: p::SchemaVersion(1),
        index,
        run: p::RunId(format!("run:m3-b:{index}")),
        checkpoint: p::GoalCheckpointRef(format!("checkpoint:m3-b:{index}")),
        snapshot: p::EvolutionSnapshotRef(snapshot.into()),
        strategy_versions: vec![
            p::StrategyVersionRef(loop_version.into()),
            p::StrategyVersionRef("coordination:m3-b:v1".into()),
            p::StrategyVersionRef("capability-selection:m3-b:v1".into()),
            p::StrategyVersionRef("model-selection:m3-b:v1".into()),
            p::StrategyVersionRef("backend-selection:m3-b:v1".into()),
            p::StrategyVersionRef("model-adaptation:m3-b:v1".into()),
        ],
        status: p::RunStatus::Complete,
        outward_run: outward.then(|| p::RunId("run:m3-b:outward".into())),
        outward_event_refs: if outward {
            (0..7)
                .map(|position| p::EventId(format!("event:m3-b:outward:{position}")))
                .collect()
        } else {
            Vec::new()
        },
        outward_event_kinds: if outward {
            vec![
                p::EventKind::ApprovalRequested,
                p::EventKind::ApprovalResolved,
                p::EventKind::CompetenceGateEvaluated,
                p::EventKind::ActionPlanned,
                p::EventKind::ActionStarted,
                p::EventKind::ActionCompleted,
                p::EventKind::VerificationFinished,
            ]
        } else {
            Vec::new()
        },
        event_refs,
        event_kinds,
    }
}

fn body() -> LongHorizonArtifactBody {
    LongHorizonArtifactBody {
        schema_version: p::SchemaVersion(1),
        goal_frame: p::GoalFrameRef("goal-frame:m3-b-long".into()),
        scope: p::Scope("workspace:m3-b".into()),
        maximum_checkpoints: 3,
        disposition: LongHorizonArtifactDisposition::Cancelled,
        checkpoints: vec![
            checkpoint(0, "snapshot:m3-b:v1", "loop:m3-b:v1", false, true),
            checkpoint(1, "snapshot:m3-b:v2", "loop:m3-b:v2", true, false),
            checkpoint(2, "snapshot:m3-b:v1-restored", "loop:m3-b:v1", false, false),
        ],
    }
}

#[test]
fn s62_long_horizon_artifact_is_content_addressed_and_offline_verifiable() {
    let root = temp_root("pass");
    let store = M3BArtifactStore::new(&root).unwrap();
    let receipt = store.write(&body()).unwrap();
    assert!(receipt.digest.0.starts_with("fnv64:"));
    assert_eq!(store.verify_complete_set().unwrap(), receipt);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn s62_artifact_rejects_tamper_extra_entries_secrets_and_private_paths() {
    let root = temp_root("tamper");
    let store = M3BArtifactStore::new(&root).unwrap();
    let receipt = store.write(&body()).unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt.path).unwrap()).unwrap();
    value["body"]["checkpoints"][1]["snapshot"] =
        serde_json::Value::String("snapshot:m3-b:tampered".into());
    fs::write(&receipt.path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    assert!(store.verify(&receipt.path).is_err());
    fs::remove_dir_all(&root).unwrap();

    let root = temp_root("extra");
    let store = M3BArtifactStore::new(&root).unwrap();
    store.write(&body()).unwrap();
    fs::write(root.join("unexpected.txt"), b"unexpected").unwrap();
    assert!(store.verify_complete_set().is_err());
    fs::remove_dir_all(&root).unwrap();

    let root = temp_root("secret");
    let store = M3BArtifactStore::new(&root).unwrap();
    let mut secret = body();
    secret.checkpoints[0].event_refs[0] = p::EventId("credential:raw-value".into());
    assert!(store.write(&secret).is_err());
    let mut private_path = body();
    private_path.scope = p::Scope("C:\\Users\\owner\\private".into());
    assert!(store.write(&private_path).is_err());
    fs::remove_dir_all(root).unwrap();
}
