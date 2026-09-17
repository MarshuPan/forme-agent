use forme_protocol as p;

fn app_api_spec() -> p::AppApiActionSpec {
    p::AppApiActionSpec {
        schema_version: p::SchemaVersion(1),
        connector: p::ProviderId("connector:project-loopback".into()),
        endpoint: "http://127.0.0.1:38191/items".into(),
        schema_digest: p::SchemaDigest("sha256:project-schema-v1".into()),
        credential: Some(p::SecretRef("secret:connector-token".into())),
        operation: p::AppApiOperation::Mutation {
            method: p::AppApiMutationMethod::Post,
            body: Some(p::ExternalInput::Content(p::ContentRef(
                "content:outbound-message".into(),
            ))),
            idempotency_key: p::IdempotencyKey("delivery:message-1".into()),
        },
        timeout: p::DurationMs(2_000),
        participant: Some(p::ParticipantId("external:person-a".into())),
        representation: Some(p::Representation::AgentRepresentingOwner),
        disclosure_request: Some(p::DisclosureRequestRef("disclosure:message-1".into())),
    }
}

#[test]
fn m2_b_app_api_contract_is_additive_versioned_and_secret_ref_only() {
    let parameters = p::ActionParameters::AppApi(app_api_spec());
    let encoded = serde_json::to_value(&parameters).unwrap();
    let decoded: p::ActionParameters = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, parameters);

    let text = serde_json::to_string(&encoded).unwrap();
    assert!(text.contains("secret:connector-token"));
    assert!(!text.contains("Authorization"));
    assert!(!text.contains("Bearer "));
    assert_eq!(p::EventKind::ALL.len(), 99);
    assert_eq!(p::EventKind::ALL[85], p::EventKind::ComplianceCheckResult);
}

#[test]
fn m2_b_connector_enums_activate_without_reinterpreting_old_values() {
    assert_eq!(
        serde_json::from_value::<p::BackendKind>(serde_json::json!("AppApi")).unwrap(),
        p::BackendKind::AppApi
    );
    assert_eq!(
        serde_json::from_value::<p::ProviderKind>(serde_json::json!("AppApi")).unwrap(),
        p::ProviderKind::AppApi
    );
    assert_eq!(
        serde_json::from_value::<p::ConfigCheck>(serde_json::json!("Connector")).unwrap(),
        p::ConfigCheck::Connector
    );
    assert_eq!(
        serde_json::from_value::<p::ConfigCheck>(serde_json::json!("Sync")).unwrap(),
        p::ConfigCheck::Sync
    );
}

#[test]
fn m2_b_app_api_invalid_boundaries_remain_explicit_for_runtime_recheck() {
    let mut missing_schema = app_api_spec();
    missing_schema.schema_version = p::SchemaVersion(0);
    let encoded = serde_json::to_value(p::ActionParameters::AppApi(missing_schema)).unwrap();
    let decoded: p::ActionParameters = serde_json::from_value(encoded).unwrap();
    let p::ActionParameters::AppApi(spec) = decoded else {
        panic!("AppApi parameters changed variant");
    };
    assert_eq!(spec.schema_version, p::SchemaVersion(0));
    assert!(spec.credential.is_some());

    let mut legacy_same_wave =
        serde_json::to_value(p::ActionParameters::AppApi(app_api_spec())).unwrap();
    legacy_same_wave["AppApi"]
        .as_object_mut()
        .unwrap()
        .remove("disclosure_request");
    let decoded: p::ActionParameters = serde_json::from_value(legacy_same_wave).unwrap();
    let p::ActionParameters::AppApi(spec) = decoded else {
        panic!("AppApi parameters changed variant");
    };
    assert!(spec.disclosure_request.is_none());
}

#[test]
fn m2_b_disclosure_binding_is_additive_and_legacy_events_cannot_invent_it() {
    let legacy = serde_json::json!({
        "request": "disclosure:legacy",
        "outcome": "Answer",
        "representation": "Agent"
    });
    let decoded: p::DisclosurePolicyAppliedPayload = serde_json::from_value(legacy).unwrap();
    assert!(decoded.binding.is_none());

    let current = p::DisclosurePolicyAppliedPayload {
        request: p::DisclosureRequestRef("disclosure:message-1".into()),
        outcome: p::DisclosureOutcome::Answer,
        representation: p::Representation::Agent,
        binding: Some(p::DisclosureBinding {
            schema_version: p::SchemaVersion(1),
            session: p::CommunicationSessionId("communication:bounded".into()),
            participant: p::ParticipantId("external:person-a".into()),
            purpose: p::PurposeRef("bounded-support".into()),
            content_ref: p::ContentRef("content:message-1".into()),
            category: "public".into(),
            sensitive: false,
            confirmed: true,
            high_impact: false,
        }),
    };
    let encoded = serde_json::to_value(&current).unwrap();
    assert_eq!(
        serde_json::from_value::<p::DisclosurePolicyAppliedPayload>(encoded).unwrap(),
        current
    );
}
