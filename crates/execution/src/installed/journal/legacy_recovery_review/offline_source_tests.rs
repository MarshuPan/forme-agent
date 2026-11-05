//! Actual offline reads: original bytes, custody and lock behavior.
use super::*;
use crate::installed::{
    journal::offline_source::{inspect_at, LegacyRepairSourceSnapshot},
    management_repair::storage::ReadCustody,
};

fn observe(path: &std::path::Path) -> p::Result<LegacyRepairSourceSnapshot> {
    inspect_at(
        &mut ReadCustody::default(),
        path,
        INSTALL,
        &hash("installation"),
        ORIGINAL,
        10_001,
    )
}

fn unlocked() -> (tempfile::TempDir, PathBuf, Vec<u8>, (u64, p::SchemaDigest)) {
    let Fixture {
        _root,
        path,
        journal,
        current,
    } = Fixture::new();
    let bytes = journal_bytes(&journal);
    drop(journal);
    (_root, path, bytes, current)
}

#[test]
fn offline_original_reports_actual_checkpoint_without_writing_or_exposing_tokens() {
    let (_root, path, bytes, checkpoint) = unlocked();
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let snapshot = observe(&path).unwrap();
    assert_eq!(snapshot.journal_seq, checkpoint.0);
    assert_eq!(snapshot.journal_head, checkpoint.1);
    assert_eq!(snapshot.journal_digest, p::sha256_content_digest(&bytes));
    assert_eq!(snapshot.journal_bytes, bytes.len() as u64);
    assert_eq!(snapshot.journal_schema_floor, 1);
    assert_eq!(snapshot.owner.0, OWNER);
    assert_eq!(snapshot.stage, p::MaintenanceStage::Validated);
    assert_eq!(snapshot.pending_stage, Some(p::MaintenanceStage::Quiescing));
    assert!(!snapshot.images_claimed);
    assert!(!snapshot.checking_prepared);
    assert!(!snapshot.recovery_baseline_present);
    let encoded = serde_json::to_string(&snapshot).unwrap();
    for private in [
        "readiness_nonce",
        "proof_ref",
        "actor_proof",
        "paths",
        "password",
    ] {
        assert!(!encoded.contains(private));
    }
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before
    );
}

#[test]
fn offline_original_refuses_live_writer_and_holds_its_own_lock_until_drop() {
    let f = Fixture::new();
    let before = journal_bytes(&f.journal);
    assert!(observe(&f.path).is_err());
    assert_eq!(journal_bytes(&f.journal), before);
    drop(f.journal);
    let mut custody = ReadCustody::default();
    let snapshot = inspect_at(
        &mut custody,
        &f.path,
        INSTALL,
        &hash("installation"),
        ORIGINAL,
        10_001,
    )
    .unwrap();
    assert!(ProductJournal::open(&f.path, INSTALL).is_err());
    drop(custody);
    let reopened = ProductJournal::open(&f.path, INSTALL).unwrap();
    assert_eq!(reopened.revision().unwrap(), snapshot.journal_seq);
    assert_eq!(journal_bytes(&reopened), before);
}

#[test]
fn offline_original_custody_blocks_replacement_or_detects_changed_path_identity() {
    let (root, path, bytes, _) = unlocked();
    let mut custody = ReadCustody::default();
    inspect_at(
        &mut custody,
        &path,
        INSTALL,
        &hash("installation"),
        ORIGINAL,
        10_001,
    )
    .unwrap();
    let moved = root.path().join("moved.journal");
    #[cfg(windows)]
    {
        assert!(std::fs::rename(&path, &moved).is_err());
        assert!(custody.recheck_records().is_ok());
        drop(custody);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(&path, &moved).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(custody.recheck_records().is_err());
        assert_eq!(std::fs::read(&moved).unwrap(), bytes);
    }
}

#[test]
fn offline_original_never_creates_missing_or_initializes_empty_files() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("original.journal");
    assert!(observe(&path).is_err());
    assert!(!path.exists());
    std::fs::write(&path, []).unwrap();
    assert!(observe(&path).is_err());
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn offline_original_refuses_hardlink_aliases_and_foreign_identity_without_changes() {
    let (root, path, bytes, _) = unlocked();
    for mode in 0..4 {
        let result = inspect_at(
            &mut ReadCustody::default(),
            &path,
            if mode == 0 {
                "install:foreign"
            } else {
                INSTALL
            },
            &hash(if mode == 1 {
                "foreign installation"
            } else {
                "installation"
            }),
            if mode == 2 {
                "operation:foreign"
            } else {
                ORIGINAL
            },
            if mode == 3 { -1 } else { 10_001 },
        );
        assert!(result.is_err(), "mode {mode}");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    let alias = root.path().join("alias.journal");
    std::fs::hard_link(&path, &alias).unwrap();
    assert!(observe(&path).is_err());
    assert!(observe(&alias).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

fn rewritten(bytes: &[u8], alter: impl FnOnce(&mut Vec<serde_json::Value>)) -> Vec<u8> {
    let mut lines: Vec<serde_json::Value> = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    alter(&mut lines);
    let mut previous = p::canonical_digest(&"forme-product-journal-v1").unwrap();
    let mut output = Vec::new();
    for line in &mut lines {
        line["previous"] = serde_json::json!(previous);
        let digest = p::canonical_digest(&(
            line["schema_version"].as_u64().unwrap(),
            line["product_seq"].as_u64().unwrap(),
            &previous,
            &line["fact"],
        ))
        .unwrap();
        line["digest"] = serde_json::json!(digest);
        output.extend(serde_json::to_vec(line).unwrap());
        output.push(b'\n');
        previous = digest;
    }
    output
}

#[test]
fn offline_original_replays_historical_absent_defaults_using_original_fact_bytes() {
    let (_root, path, bytes, checkpoint) = unlocked();
    let historical = rewritten(&bytes, |lines| {
        for line in lines {
            if line["fact"]["record"] == "planned" {
                line["fact"]
                    .as_object_mut()
                    .unwrap()
                    .remove("network_contract");
            }
        }
    });
    std::fs::write(&path, &historical).unwrap();
    let snapshot = observe(&path).unwrap();
    assert_eq!(snapshot.journal_seq, checkpoint.0);
    assert_eq!(
        snapshot.journal_digest,
        p::sha256_content_digest(&historical)
    );
    assert_eq!(std::fs::read(&path).unwrap(), historical);
}

#[test]
fn offline_original_rejects_corruption_unknown_schema_and_invalid_causality_without_repair() {
    let (_root, path, bytes, _) = unlocked();
    let mut altered_digest = bytes.clone();
    altered_digest[10] ^= 1;
    let cases = [
        bytes[..bytes.len() - 1].to_vec(),
        altered_digest,
        rewritten(&bytes, |lines| {
            lines.last_mut().unwrap()["schema_version"] = 99.into()
        }),
        rewritten(&bytes, |lines| {
            lines.last_mut().unwrap()["product_seq"] = 99.into()
        }),
        rewritten(&bytes, |lines| {
            lines.last_mut().unwrap()["fact"]["stage"] = "switching".into()
        }),
        rewritten(&bytes, |lines| lines[0]["unexpected"] = true.into()),
    ];
    for (index, invalid) in cases.into_iter().enumerate() {
        std::fs::write(&path, &invalid).unwrap();
        assert!(observe(&path).is_err(), "case {index}");
        assert_eq!(std::fs::read(&path).unwrap(), invalid);
    }
}
