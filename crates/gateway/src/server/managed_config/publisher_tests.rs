//! Native composition must consume Host publisher files before serving controls.
use super::*;
use forme_harness::EcosystemGatewayControl;

#[test]
fn managed_publisher_configuration_is_available_after_runtime_restart() {
    let temp = Temporary::new();
    let mut config = temp.configured();
    let bytes = ed25519_dalek::SigningKey::from_bytes(&[93; 32])
        .verifying_key()
        .to_bytes();
    let key_path = temp.0.join("publisher.key");
    std::fs::write(&key_path, bytes).unwrap();
    let digest = p::sha256_content_digest(&bytes);
    config.brain.managed_capabilities = Some(p::ProductManagedCapabilitySettings {
        publisher_keys: vec![p::ProductPublisherKeyView {
            schema_version: p::SchemaVersion(1),
            publisher_ref: "publisher:managed".into(),
            public_key_digest: digest.clone(),
            configured: true,
        }],
        ..Default::default()
    });
    config
        .brain
        .publisher_key_files
        .insert("publisher:managed".into(), key_path.clone());
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.brain.clone())
        .unwrap()
        .build()
        .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let grant = p::CapabilityPublisherGrant {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityPublisherGrantRef("grant:managed".into()),
        publisher: p::CapabilityPublisherRef("publisher:managed".into()),
        public_key_digest: digest,
        allowed_kinds: vec![p::CapabilityPackageKind::Skill],
        scope: p::Scope(config.brain.workspace.0.clone()),
        expires_at: now + 60_000,
        version: p::Version(1),
        status: p::CapabilityPublisherStatus::Active,
    };
    runtime
        .harness()
        .provision_capability_publisher(
            p::RunId("run:publisher".into()),
            grant,
            None,
            p::EcosystemAggregateVersion::zero(),
            p::VerifiedPrincipal(config.brain.owner.0.clone()),
        )
        .unwrap();
    drop(runtime);
    let reopened = ProductionRuntimeBuilder::from_managed_config(config.brain.clone())
        .unwrap()
        .build()
        .unwrap();
    let snapshot = reopened
        .harness()
        .capability_ecosystem_snapshot(p::Scope(config.brain.workspace.0.clone()))
        .unwrap();
    assert_eq!(snapshot.publishers.len(), 1);
    drop(reopened);
    std::fs::write(&key_path, [1_u8; 32]).unwrap();
    assert!(ProductionRuntimeBuilder::from_managed_config(config.brain.clone()).is_err());
    std::fs::remove_file(&key_path).unwrap();
    assert!(ProductionRuntimeBuilder::from_managed_config(config.brain).is_err());
}
