use forme_protocol as p;

fn digest(value: &str) -> p::SchemaDigest {
    p::sha256_content_digest(value.as_bytes())
}
fn takeover() -> p::ProductReleaseTakeover {
    p::ProductReleaseTakeover {
        schema_version: p::SchemaVersion(1),
        release: p::ProductReleaseRequest {
            schema_version: p::SchemaVersion(1),
            install_ref: "installation:test".into(),
            operation_id: "upgrade:test".into(),
            command_digest: digest("command"),
            change: p::ProductReleaseChange::Upgrade,
            original_release_ref: "0.0.1".into(),
            original_release_digest: digest("old"),
            target_release_ref: "0.0.2".into(),
            target_release_digest: digest("new"),
            backup_ref: "backup:test".into(),
        },
        installation_digest: digest("installation"),
        owner: p::OwnerPrincipalRef("owner:test".into()),
        expected_config_revision: 1,
        expected_config_digest: digest("configuration"),
        deadline: 1000,
        readiness_nonce: "a".repeat(64),
    }
}
fn binding(takeover: &p::ProductReleaseTakeover) -> p::ProductReleaseBinding {
    p::ProductReleaseBinding {
        schema_version: p::SchemaVersion(1),
        install_ref: takeover.release.install_ref.clone(),
        operation_id: takeover.release.operation_id.clone(),
        command_digest: takeover.release.command_digest.clone(),
        target_release_ref: takeover.release.target_release_ref.clone(),
        target_release_digest: takeover.release.target_release_digest.clone(),
        core_image_digest: digest("image"),
        owner_digest: p::sha256_content_digest(takeover.owner.0.as_bytes()),
        store_identity: digest("store"),
        config_revision: 2,
        readiness_nonce: takeover.readiness_nonce.clone(),
    }
}

#[test]
fn takeover_is_closed_versioned_and_bound_to_the_original_configuration() {
    let takeover = takeover();
    takeover.validate().unwrap();
    let encoded = serde_json::to_vec(&takeover).unwrap();
    assert_eq!(
        serde_json::from_slice::<p::ProductReleaseTakeover>(&encoded).unwrap(),
        takeover
    );
    let mut extra = serde_json::to_value(&takeover).unwrap();
    extra["program"] = "replacement".into();
    assert!(serde_json::from_value::<p::ProductReleaseTakeover>(extra).is_err());
    for case in 0..4 {
        let mut bad = takeover.clone();
        match case {
            0 => bad.schema_version = p::SchemaVersion(2),
            1 => bad.readiness_nonce = "A".repeat(64),
            2 => bad.expected_config_revision = 0,
            _ => bad.release.target_release_ref = bad.release.original_release_ref.clone(),
        }
        assert!(bad.validate().is_err());
    }
}

#[test]
fn admission_and_readiness_cannot_be_reused_for_another_release_or_runtime() {
    let takeover = takeover();
    let binding = binding(&takeover);
    binding.validate_for(&takeover).unwrap();
    let admission = p::ProductReleaseAdmission {
        schema_version: p::SchemaVersion(1),
        binding_digest: binding.digest().unwrap(),
        released: true,
        at: 100,
    };
    admission.validate_for(&binding).unwrap();
    let ready = p::ProductReleaseReadiness {
        schema_version: p::SchemaVersion(1),
        binding: binding.clone(),
        config_revision: 2,
        actual_image_digest: binding.core_image_digest.clone(),
        data_identity: p::ProductDataAcknowledgement {
            schema_version: p::SchemaVersion(1),
            owner_digest: binding.owner_digest.clone(),
            store_identity: binding.store_identity.clone(),
            execution_fence_digest: None,
            composition_digest: digest("composition"),
        },
    };
    ready.validate().unwrap();
    for case in 0..7 {
        let mut changed = binding.clone();
        match case {
            0 => changed.operation_id = "upgrade:other".into(),
            1 => changed.command_digest = digest("other"),
            2 => changed.target_release_digest = digest("other"),
            3 => changed.config_revision = 3,
            4 => changed.readiness_nonce = "b".repeat(64),
            5 => changed.owner_digest = digest("other"),
            _ => changed.install_ref = "installation:other".into(),
        }
        assert!(changed.validate_for(&takeover).is_err());
        assert!(admission.validate_for(&changed).is_err());
    }
    for case in 0..4 {
        let mut changed = ready.clone();
        match case {
            0 => changed.actual_image_digest = digest("other"),
            1 => changed.data_identity.store_identity = digest("other"),
            2 => changed.data_identity.owner_digest = digest("other"),
            _ => changed.config_revision = 1,
        }
        assert!(changed.validate().is_err());
    }
}
