use forme_protocol as p;

fn browser_spec() -> p::BrowserActionSpec {
    p::BrowserActionSpec {
        schema_version: p::SchemaVersion(1),
        driver: p::ProviderId("driver:headless-chrome".into()),
        target_url: "http://127.0.0.1:34001/task".into(),
        allowed_origins: vec!["http://127.0.0.1:34001".into()],
        operation: p::BrowserOperation::Click {
            selector: "#commit".into(),
        },
        artifact_scope: p::Scope("workspace:m2-golden".into()),
    }
}

#[test]
fn m2_a_protocol_objects_are_additive_versioned_and_round_trip() {
    let browser = p::ActionParameters::Browser(browser_spec());
    let computer = p::ActionParameters::Computer(p::ComputerActionSpec {
        schema_version: p::SchemaVersion(1),
        driver: p::ProviderId("driver:recording-computer".into()),
        surface: p::SurfaceRef("surface:m2-isolated".into()),
        bounds: p::CoordinateBounds {
            schema_version: p::SchemaVersion(1),
            min_x: 0,
            min_y: 0,
            max_x_exclusive: 800,
            max_y_exclusive: 600,
        },
        operation: p::ComputerOperation::Click {
            x: 40,
            y: 60,
            button: p::PointerButton::Primary,
        },
        artifact_scope: p::Scope("workspace:m2-golden".into()),
    });
    let pty = p::ActionParameters::Pty(p::PtyActionSpec {
        schema_version: p::SchemaVersion(1),
        program: "fixture-pty".into(),
        args: vec!["echo".into()],
        cwd: "C:/fixture".into(),
        cols: 80,
        rows: 24,
        input: Some(p::ExternalInput::Content(p::ContentRef(
            "content:pty-input".into(),
        ))),
        environment: vec![p::SecretBinding {
            schema_version: p::SchemaVersion(1),
            name: "FIXTURE_TOKEN".into(),
            value: p::SecretRef("secret:pty-fixture".into()),
        }],
    });

    for parameters in [browser, computer, pty] {
        let encoded = serde_json::to_value(&parameters).unwrap();
        assert_eq!(
            serde_json::from_value::<p::ActionParameters>(encoded).unwrap(),
            parameters
        );
    }
    assert_eq!(p::EventKind::ALL.len(), 99);
    assert_eq!(p::EventKind::ALL[85], p::EventKind::ComplianceCheckResult);
}

#[test]
fn m2_a_legacy_action_events_decode_with_safe_defaults() {
    let output: p::ActionOutputDeltaPayload = serde_json::from_value(serde_json::json!({
        "intent_id": "action:legacy",
        "backend": "Shell",
        "scope": "workspace:legacy",
        "delta": "legacy output",
        "truncated": false
    }))
    .unwrap();
    assert_eq!(output.trust, p::TrustTier::Untrusted);
    assert!(output.content_ref.is_none());

    let completed: p::ActionCompletedPayload = serde_json::from_value(serde_json::json!({
        "intent_id": "action:legacy",
        "result_ref": "result:legacy"
    }))
    .unwrap();
    assert!(completed.receipt.is_none());
}

#[test]
fn m2_a_external_receipt_is_typed_and_untrusted() {
    let payload = p::ActionCompletedPayload {
        intent_id: p::ActionId("action:browser".into()),
        result_ref: p::ActionResultRef("result:browser".into()),
        receipt: Some(p::ExternalActionReceipt {
            schema_version: p::SchemaVersion(1),
            action: p::ActionId("action:browser".into()),
            content_ref: Some(p::ContentRef("artifact:browser-text".into())),
            content_digest: Some(p::SchemaDigest("sha256:fixture".into())),
            trust: p::TrustTier::Untrusted,
            effect: p::EffectStatus::Committed,
            probe_hint: None,
        }),
        remote_receipt: None,
    };
    let encoded = serde_json::to_value(&payload).unwrap();
    assert_eq!(encoded["receipt"]["trust"], "Untrusted");
    assert_eq!(
        serde_json::from_value::<p::ActionCompletedPayload>(encoded).unwrap(),
        payload
    );
}

#[test]
fn m2_a_zero_and_unknown_boundaries_fail_closed() {
    let bounds = p::CoordinateBounds {
        schema_version: p::SchemaVersion(0),
        min_x: 0,
        min_y: 0,
        max_x_exclusive: 0,
        max_y_exclusive: 0,
    };
    assert!(bounds.validate().is_err());
    assert!(!bounds.contains(0, 0));

    assert!(serde_json::from_value::<p::BackendKind>(serde_json::json!("FutureBackend")).is_err());
    assert_eq!(
        serde_json::from_value::<p::ConfigCheck>(serde_json::json!("Sync")).unwrap(),
        p::ConfigCheck::Sync
    );
}
