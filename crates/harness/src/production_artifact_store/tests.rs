use super::*;
use forme_store::{BrainAuthorityClaim, BrainAuthorityStore, FileContentKeyResolver, StoreOptions};
use std::sync::Arc;

#[test]
fn encrypted_artifact_receipts_bind_original_material() {
    let root = std::env::temp_dir().join(format!(
        "forme-artifact-metadata-{}-{}",
        std::process::id(),
        crate::now_ms()
    ));
    std::fs::create_dir(&root).unwrap();
    let owner = p::VerifiedPrincipal("owner:artifact-metadata".into());
    let scope = p::Scope("workspace:artifact-metadata".into());
    let store = SqliteEventStore::open(root.join("brain.db"), StoreOptions::default()).unwrap();
    let keys = Arc::new(FileContentKeyResolver::new(root.join("keys")).unwrap());
    let protection = StoreContentProtection::new(&owner, scope.clone(), keys).unwrap();
    store.enable_content_protection(protection.clone()).unwrap();
    let authority = store
        .claim_brain_authority(BrainAuthorityClaim {
            owner_digest: p::sha256_content_digest(owner.0.as_bytes()),
            process_nonce: p::Nonce("nonce:artifact-metadata-regression".into()),
            now_ms: crate::now_ms(),
            lease_ttl: p::DurationMs(60_000),
        })
        .unwrap();
    let guard = store
        .install_authority_lease_guard(&authority, p::DurationMs(60_000))
        .unwrap();
    let sink = ProtectedRuntimeArtifacts {
        inner: FileArtifactStore::new(root.join("artifacts")).unwrap(),
        protection: protection.clone(),
        store: store.clone(),
        owner: owner.clone(),
    };
    let inputs: &[&[u8]] = &[
        b"\x89PNG\r\n\x1a\nself-authored-binary-artifact-957da",
        b"self-authored-text-artifact-7a3ce",
    ];
    let service = protection.lifecycle_service(store.clone());
    for &original in inputs {
        let first = sink.write(&scope, "bin", original).unwrap();
        let second = sink.write(&scope, "bin", original).unwrap();
        assert_ne!(first.content_ref, second.content_ref);
        for receipt in [first, second] {
            assert_eq!(receipt.schema_version, p::SchemaVersion(1));
            assert_eq!(receipt.digest, p::sha256_content_digest(original));
            assert_eq!(receipt.bytes, original.len() as u64);
            let record = service
                .protected_content(&owner, &receipt.content_ref)
                .unwrap()
                .unwrap();
            assert_eq!(
                protection
                    .content_service()
                    .decrypt(&record.envelope.binding(), &record.envelope)
                    .unwrap(),
                original
            );
            let material = read_material(&store, &receipt.content_ref, &scope).unwrap();
            assert_eq!(material.digest, Some(receipt.digest));
            assert_eq!(material.bytes, Some(receipt.bytes));
        }
    }
    let mut sealed_count = 0;
    for directory in std::fs::read_dir(sink.inner.root()).unwrap() {
        for file in std::fs::read_dir(directory.unwrap().path()).unwrap() {
            let sealed = std::fs::read(file.unwrap().path()).unwrap();
            for &original in inputs {
                assert!(!sealed
                    .windows(original.len())
                    .any(|bytes| bytes == original));
                assert_ne!(
                    p::sha256_content_digest(&sealed),
                    p::sha256_content_digest(original)
                );
            }
            sealed_count += 1;
        }
    }
    assert_eq!(sealed_count, 4);
    drop(service);
    drop(sink);
    drop(guard);
    drop(store);
    drop(protection);
    std::fs::remove_dir_all(root).unwrap();
}
