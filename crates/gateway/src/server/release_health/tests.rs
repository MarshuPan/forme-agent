use super::*;

fn readiness() -> p::ProductReleaseReadiness {
    let hash = |s: &str| p::sha256_content_digest(s.as_bytes());
    let binding = p::ProductReleaseBinding {
        schema_version: p::SchemaVersion(1),
        install_ref: "install:live-test".into(),
        operation_id: "upgrade:live-test".into(),
        command_digest: hash("command"),
        target_release_ref: "0.0.2".into(),
        target_release_digest: hash("release"),
        core_image_digest: hash("image"),
        owner_digest: hash("owner:test"),
        store_identity: hash("store"),
        config_revision: 2,
        readiness_nonce: "a".repeat(64),
    };
    p::ProductReleaseReadiness {
        schema_version: p::SchemaVersion(1),
        config_revision: 2,
        actual_image_digest: binding.core_image_digest.clone(),
        data_identity: p::ProductDataAcknowledgement {
            schema_version: p::SchemaVersion(1),
            owner_digest: binding.owner_digest.clone(),
            store_identity: binding.store_identity.clone(),
            execution_fence_digest: None,
            composition_digest: hash("composition"),
        },
        binding,
    }
}
fn signed(key: &SigningKey) -> p::ProductReleaseReadinessProbe {
    let mut probe = p::ProductReleaseReadinessProbe {
        schema_version: p::SchemaVersion(1),
        install_ref: "install:live-test".into(),
        nonce: "b".repeat(64),
        signature: Vec::new(),
    };
    probe.signature = key
        .sign(probe.signing_digest().unwrap().0.as_bytes())
        .to_bytes()
        .to_vec();
    probe
}

#[test]
fn proof_is_live_nonce_bound_signed_and_uses_a_different_signature_domain() {
    let token = LocalBearerSecret::new("c".repeat(64)).unwrap();
    let seed = p::product_release_probe_seed(token.0.as_bytes()).unwrap();
    let key = SigningKey::from_bytes(&seed);
    let health = ReleaseHealth::new(readiness(), &token).unwrap();
    let mut probe = signed(&key);
    let proof = health.prove(&probe).unwrap();
    proof.validate_for(&probe).unwrap();
    key.verifying_key()
        .verify_strict(
            proof.signing_digest().unwrap().0.as_bytes(),
            &Signature::from_slice(&proof.signature).unwrap(),
        )
        .unwrap();
    assert_eq!(proof.readiness, readiness());
    assert_ne!(proof.signature, probe.signature);
    probe.nonce = "d".repeat(64);
    assert!(health.prove(&probe).is_err());
    assert!(proof.validate_for(&probe).is_err());
    let foreign = signed(&SigningKey::from_bytes(&[17; 32]));
    assert!(health.prove(&foreign).is_err());
    let mut altered = signed(&key);
    altered.install_ref = "install:other".into();
    assert!(health.prove(&altered).is_err());
}
