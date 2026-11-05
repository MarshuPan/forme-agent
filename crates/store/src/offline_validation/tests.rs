//! A deleted envelope is lineage metadata, never authenticated owner evidence.
use super::*;
use crate::{
    BrainAuthorityClaim, BrainAuthorityStore, ContentKeyResolver, EventStore,
    FileContentKeyResolver, SqliteEventStore, StoreOptions,
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let root = self.0.canonicalize().unwrap();
        assert_eq!(
            root.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        assert!(root
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("forme-deleted-owner-evidence-"));
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn crypto_deleted_envelopes_do_not_count_as_authenticated_owner_content() {
    let directory = Directory(std::env::temp_dir().join(
        format!("forme-deleted-owner-evidence-{}-{}",std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()),
    ));
    fs::create_dir(&directory.0).unwrap();
    let path = directory.0.join("brain.db");
    let owner = p::VerifiedPrincipal("owner:deleted-fixture".into());
    let scope = p::Scope("workspace:deleted-fixture".into());
    let keys = Arc::new(FileContentKeyResolver::new_managed(directory.0.join("keys")).unwrap());
    let protection = StoreContentProtection::new(&owner, scope, keys.clone()).unwrap();
    let store = SqliteEventStore::open(&path, StoreOptions::default()).unwrap();
    store
        .claim_brain_authority(BrainAuthorityClaim {
            owner_digest: p::sha256_content_digest(owner.0.as_bytes()),
            process_nonce: p::Nonce("fixture:writer".into()),
            now_ms: 1,
            lease_ttl: p::DurationMs(1),
        })
        .unwrap();
    store.enable_content_protection(protection.clone()).unwrap();
    store
        .append(p::Event::new(
            p::EventId("event:deleted".into()),
            p::RunId("run:deleted".into()),
            None,
            p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                source: p::Source::UserTurn,
                session_ref: p::SessionId("session:deleted".into()),
                input_ref: p::InputRef("private fixture".into()),
                idempotency_key: None,
            }),
            p::SchemaVersion(1),
            1,
            p::Provenance {
                source: p::Source::UserTurn,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
        ))
        .unwrap();
    drop(store);
    assert!(
        inspect(&path, &protection)
            .unwrap()
            .authenticated_content_rows
            > 0
    );
    // Construct the historical all-deleted boundary without a new audit event:
    // a newly encrypted audit row would itself authenticate the owner.
    let connection = Connection::open(&path).unwrap();
    let rows: Vec<(String, String)> = connection
        .prepare("SELECT object_ref,key_digest FROM protected_rows")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(!rows.is_empty());
    for (object, key) in rows {
        connection
            .execute(
                "INSERT OR IGNORE INTO data_lifecycle_deleted_objects(object_ref) VALUES(?1)",
                [object],
            )
            .unwrap();
        keys.destroy(&p::SchemaDigest(key.clone())).unwrap();
        assert!(keys.resolve(&p::SchemaDigest(key)).is_err());
    }
    drop(connection);
    let before = fs::read(&path).unwrap();
    assert_eq!(
        inspect(&path, &protection)
            .unwrap()
            .authenticated_content_rows,
        0
    );
    assert_eq!(fs::read(&path).unwrap(), before);
}
