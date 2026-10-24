use forme_protocol as p;

#[test]
fn service_image_receipts_require_all_three_distinct_services_and_the_original_command() {
    let request = request().release;
    let receipt = p::ProductReleaseServiceReceipt {
        schema_version: p::SchemaVersion(1),
        operation_id: request.operation_id.clone(),
        command_digest: request.command_digest.clone(),
        images: [
            p::BuiltinServiceId::Web,
            p::BuiltinServiceId::Brain,
            p::BuiltinServiceId::Host,
        ]
        .into_iter()
        .map(|service| p::ProductReleaseServiceImageReceipt {
            schema_version: p::SchemaVersion(1),
            service,
            source_image_digest: p::canonical_digest(&(service, "source")).unwrap(),
            target_image_digest: p::canonical_digest(&(service, "target")).unwrap(),
            configuration_digest: p::canonical_digest(&(service, "native observation")).unwrap(),
        })
        .collect(),
    };
    receipt.validate_for(&request).unwrap();
    let mut incomplete = receipt.clone();
    incomplete.images.pop();
    assert!(incomplete.validate_for(&request).is_err());
    let mut duplicate = receipt.clone();
    duplicate.images[1] = duplicate.images[0].clone();
    assert!(duplicate.validate_for(&request).is_err());
    let mut other = receipt.clone();
    other.command_digest = p::canonical_digest(&"another command").unwrap();
    assert!(other.validate_for(&request).is_err());
    let mut version = receipt.clone();
    version.images[1].schema_version = p::SchemaVersion(99);
    assert!(version.validate_for(&request).is_err());
    let mut value = serde_json::to_value(receipt).unwrap();
    value["images"][0]["shell"] = "disallowed".into();
    assert!(serde_json::from_value::<p::ProductReleaseServiceReceipt>(value).is_err());
}

fn request() -> p::ProductReleaseDataRequest {
    p::ProductReleaseDataRequest {
        schema_version: p::SchemaVersion(1),
        expected_config_revision: 1,
        owner: p::OwnerPrincipalRef("owner:release-data".into()),
        release: p::ProductReleaseRequest {
            schema_version: p::SchemaVersion(1),
            install_ref: "install:release-data".into(),
            operation_id: "operation:release-data".into(),
            command_digest: p::canonical_digest(&"command").unwrap(),
            change: p::ProductReleaseChange::Upgrade,
            original_release_ref: "0.1.0".into(),
            original_release_digest: p::canonical_digest(&"old").unwrap(),
            target_release_ref: "0.2.0".into(),
            target_release_digest: p::canonical_digest(&"new").unwrap(),
            backup_ref: "backup:data".into(),
        },
    }
}

#[test]
fn release_data_identity_is_bound_to_owner_operation_and_real_schema_versions() {
    let request = request();
    let identity = p::ProductReleaseDataIdentity {
        schema_version: p::SchemaVersion(1),
        config_schema_version: 1,
        database_schema_version: 13,
        owner_digest: p::sha256_content_digest(request.owner.0.as_bytes()),
        store_identity: p::canonical_digest(&"store").unwrap(),
        schema_digest: p::canonical_digest(&"schema").unwrap(),
        database_digest: p::canonical_digest(&"database").unwrap(),
        event_count: 12,
        last_stream_seq: 6,
    };
    let response = p::ProductReleaseDataResponse {
        schema_version: p::SchemaVersion(1),
        operation_id: request.release.operation_id.clone(),
        command_digest: request.release.command_digest.clone(),
        result: p::ProductReleaseDataResult::Inspected { identity },
    };
    response.validate_for(&request).unwrap();
    for change in 0..4 {
        let mut wrong = response.clone();
        match change {
            0 => wrong.operation_id = "operation:other".into(),
            1 => wrong.command_digest = p::canonical_digest(&"other").unwrap(),
            _ => {
                let p::ProductReleaseDataResult::Inspected { identity } = &mut wrong.result else {
                    panic!()
                };
                if change == 2 {
                    identity.owner_digest = p::canonical_digest(&"other").unwrap();
                } else {
                    identity.database_schema_version = 0;
                }
            }
        }
        assert!(wrong.validate_for(&request).is_err());
    }
}

#[test]
fn release_data_protocol_is_closed_and_rejects_rebinding_and_unknown_versions() {
    let request = request();
    let mut value = serde_json::to_value(&request).unwrap();
    value["database_path"] = serde_json::json!("untrusted.db");
    assert!(serde_json::from_value::<p::ProductReleaseDataRequest>(value).is_err());
    let mut wrong = request.clone();
    wrong.schema_version = p::SchemaVersion(2);
    assert!(wrong.validate().is_err());
    let mut step = p::ProductReleaseDataStepRequest {
        schema_version: p::SchemaVersion(1),
        operation_id: request.release.operation_id.clone(),
        command_digest: request.release.command_digest.clone(),
        step: p::ProductReleaseDataStep::Backup,
    };
    step.validate_for(&request).unwrap();
    step.command_digest = p::canonical_digest(&"another operation").unwrap();
    assert!(step.validate_for(&request).is_err());
}
