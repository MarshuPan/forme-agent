use super::*;

const NOW: i64 = 1_800_000_000_000;

fn assertion(nonce: &str, epoch: u64) -> p::CoreSessionAssertion {
    p::CoreSessionAssertion {
        schema_version: p::SchemaVersion(1),
        install_ref: "install:fence-fixture".into(),
        owner: p::OwnerPrincipalRef("owner:fence-fixture".into()),
        session_digest: p::SchemaDigest(format!("sha256:{}", "a".repeat(64))),
        auth_epoch: epoch,
        audience: "forme-core".into(),
        request_digest: p::SchemaDigest(format!("sha256:{}", "b".repeat(64))),
        expires_at: NOW + 20_000,
        nonce: nonce.into(),
        signature: vec![1; 64],
    }
}

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).unwrap();
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("forme-fence-{:x}", u128::from_le_bytes(bytes)));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn path(&self) -> std::path::PathBuf {
        self.0.join("fence.sqlite")
    }
    fn open(&self) -> SessionFence {
        SessionFence::open(
            &self.path(),
            "install:fence-fixture",
            &p::VerifiedPrincipal("owner:fence-fixture".into()),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn bridge_nonce_and_session_revocation_survive_restart() {
    let fixture = Fixture::new();
    let fence = fixture.open();
    let first = assertion("nonce:one", 1);
    fence.admit(&first, NOW, false).unwrap();
    assert!(fence.admit(&first, NOW, false).is_err());
    let update = p::WebSessionFenceUpdate {
        schema_version: p::SchemaVersion(1),
        install_ref: first.install_ref.clone(),
        owner: first.owner.clone(),
        session_digest: Some(first.session_digest.clone()),
        minimum_auth_epoch: 2,
    };
    fence.apply(&update).unwrap();
    assert!(fence
        .admit(&assertion("nonce:revoked", 1), NOW, false)
        .is_err());
    // A lost fence acknowledgement may be redelivered, never as a brain action.
    fence
        .admit(&assertion("nonce:fence-retry", 1), NOW, true)
        .unwrap();
    fence.apply(&update).unwrap();
    drop(fence);
    let reopened = fixture.open();
    assert!(reopened.admit(&first, NOW, false).is_err());
    assert!(reopened
        .admit(&assertion("nonce:old", 1), NOW, false)
        .is_err());
    reopened
        .admit(&assertion("nonce:new", 2), NOW, false)
        .unwrap();
    let mut other = assertion("nonce:other-session", 1);
    other.session_digest = p::SchemaDigest(format!("sha256:{}", "c".repeat(64)));
    reopened.admit(&other, NOW, false).unwrap();
}

#[test]
fn bridge_fence_rejects_foreign_store_second_writer_expiry_and_epoch_regression() {
    let fixture = Fixture::new();
    let fence = fixture.open();
    assert!(SessionFence::open(
        &fixture.path(),
        "install:fence-fixture",
        &p::VerifiedPrincipal("owner:fence-fixture".into())
    )
    .is_err());
    let mut invalid = assertion("nonce:future", 1);
    invalid.expires_at = NOW + 30_001;
    assert!(fence.admit(&invalid, NOW, false).is_err());
    invalid.expires_at = NOW;
    assert!(fence.admit(&invalid, NOW, false).is_err());
    let first = assertion("nonce:valid", 1);
    let mut update = p::WebSessionFenceUpdate {
        schema_version: p::SchemaVersion(1),
        install_ref: first.install_ref.clone(),
        owner: first.owner.clone(),
        session_digest: None,
        minimum_auth_epoch: 3,
    };
    fence.apply(&update).unwrap();
    update.minimum_auth_epoch = 2;
    fence.apply(&update).unwrap();
    assert!(fence
        .admit(&assertion("nonce:lower", 2), NOW, false)
        .is_err());
    drop(fence);
    assert!(SessionFence::open(
        &fixture.path(),
        "install:other",
        &p::VerifiedPrincipal("owner:fence-fixture".into())
    )
    .is_err());
    assert!(SessionFence::open(
        &fixture.path(),
        "install:fence-fixture",
        &p::VerifiedPrincipal("owner:other".into())
    )
    .is_err());
    let unknown = fixture.0.join("foreign.sqlite");
    std::fs::write(&unknown, b"foreign data").unwrap();
    assert!(SessionFence::open(
        &unknown,
        "install:fence-fixture",
        &p::VerifiedPrincipal("owner:fence-fixture".into())
    )
    .is_err());
    assert_eq!(std::fs::read(&unknown).unwrap(), b"foreign data");
}

#[test]
fn bridge_nonce_consumption_is_atomic_for_competing_requests() {
    let fixture = Fixture::new();
    let fence = std::sync::Arc::new(fixture.open());
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let fence = fence.clone();
            std::thread::spawn(move || {
                fence
                    .admit(&assertion("nonce:shared", 1), NOW, false)
                    .is_ok()
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .filter(|t| t.thread().id() != std::thread::current().id())
            .map(|t| t.join().unwrap() as usize)
            .sum::<usize>(),
        1
    );
}
