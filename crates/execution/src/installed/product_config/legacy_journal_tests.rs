//! Reproduce the pre-network writer's wire shape, independent of new defaults.
use super::{journal::*, p};
use serde::Serialize;

#[derive(Clone, Serialize)]
struct LegacySnapshot {
    id: String,
    revision: u64,
    state_digest: p::SchemaDigest,
    core_digest: Option<p::SchemaDigest>,
}

#[derive(Serialize)]
struct LegacyIntent {
    operation: String,
    digest: p::SchemaDigest,
    snapshot: LegacySnapshot,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LegacyFact {
    Bound { install_ref: String },
    Imported { intent: LegacyIntent },
    Prepared { intent: LegacyIntent },
    Committed { operation: String },
    Acknowledged { revision: u64 },
}

#[derive(Serialize)]
struct LegacyEnvelope {
    schema_version: p::SchemaVersion,
    sequence: u64,
    previous: p::SchemaDigest,
    fact: LegacyFact,
    digest: p::SchemaDigest,
}

fn digest(value: &[u8]) -> p::SchemaDigest {
    p::sha256_content_digest(value)
}

fn legacy_bytes() -> Vec<u8> {
    let snapshot = LegacySnapshot {
        id: "0123456789abcdef0123456789abcdef".into(),
        revision: 0,
        state_digest: digest(b"legacy-empty-state"),
        core_digest: None,
    };
    let mut saved = snapshot.clone();
    saved.revision = 1;
    saved.core_digest = Some(digest(b"legacy-core-bytes"));
    let facts = [
        LegacyFact::Bound {
            install_ref: "install:legacy-network".into(),
        },
        LegacyFact::Imported {
            intent: LegacyIntent {
                operation: "legacy:import".into(),
                digest: digest(b"legacy-import-command"),
                snapshot,
            },
        },
        LegacyFact::Committed {
            operation: "legacy:import".into(),
        },
        LegacyFact::Prepared {
            intent: LegacyIntent {
                operation: "legacy:save".into(),
                digest: digest(b"legacy-save-command"),
                snapshot: saved,
            },
        },
        LegacyFact::Committed {
            operation: "legacy:save".into(),
        },
        LegacyFact::Acknowledged { revision: 1 },
    ];
    let mut previous = p::canonical_digest(&"forme-product-config-journal-v1").unwrap();
    let mut bytes = Vec::new();
    for (index, fact) in facts.into_iter().enumerate() {
        let sequence = index as u64 + 1;
        let current =
            p::canonical_digest(&(p::SchemaVersion(1), sequence, &previous, &fact)).unwrap();
        let entry = LegacyEnvelope {
            schema_version: p::SchemaVersion(1),
            sequence,
            previous,
            fact,
            digest: current.clone(),
        };
        bytes.extend(serde_json::to_vec(&entry).unwrap());
        bytes.push(b'\n');
        previous = current;
    }
    bytes
}

#[test]
fn pre_network_journal_bytes_replay_unchanged_and_accept_additive_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.jsonl");
    let original = legacy_bytes();
    assert!(!String::from_utf8_lossy(&original).contains("web_network_digest"));
    std::fs::write(&path, &original).unwrap();
    let journal = ConfigJournal::open(&path, "install:legacy-network").unwrap();
    assert_eq!(journal.current().unwrap().revision, 1);
    assert!(journal.current().unwrap().web_network_digest.is_none());
    assert_eq!(journal.acknowledged(), Some(1));
    assert!(journal
        .existing("legacy:save", &digest(b"legacy-save-command"))
        .unwrap());
    drop(journal);
    assert_eq!(std::fs::read(&path).unwrap(), original);

    let mut journal = ConfigJournal::open(&path, "install:legacy-network").unwrap();
    journal
        .prepare(Intent {
            operation: "network:new-snapshot".into(),
            digest: digest(b"new-network-command"),
            snapshot: SnapshotRef {
                id: "abcdef0123456789abcdef0123456789".into(),
                revision: 2,
                state_digest: digest(b"new-state"),
                core_digest: Some(digest(b"new-core")),
                web_network_digest: Some(digest(b"new-overlay")),
            },
        })
        .unwrap();
    journal.commit("network:new-snapshot".into()).unwrap();
    drop(journal);
    let mixed = std::fs::read(&path).unwrap();
    assert!(mixed.starts_with(&original));
    let journal = ConfigJournal::open(&path, "install:legacy-network").unwrap();
    assert_eq!(
        journal.current().unwrap().web_network_digest,
        Some(digest(b"new-overlay"))
    );
    assert!(journal
        .existing("legacy:import", &digest(b"legacy-import-command"))
        .unwrap());
    drop(journal);

    // Dropping the additive binding without updating its envelope remains tampering.
    let changed = String::from_utf8(mixed).unwrap().replace(
        &format!(",\"web_network_digest\":\"{}\"", digest(b"new-overlay").0),
        "",
    );
    let corrupt = directory.path().join("tampered.jsonl");
    std::fs::write(&corrupt, changed.as_bytes()).unwrap();
    assert!(ConfigJournal::open(&corrupt, "install:legacy-network").is_err());
    assert_eq!(std::fs::read(corrupt).unwrap(), changed.as_bytes());
}
