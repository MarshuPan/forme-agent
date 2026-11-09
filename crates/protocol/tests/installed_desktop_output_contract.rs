//! Metadata contract tests; these never claim to capture or decode an image.
use forme_protocol as p;
#[path = "installed_desktop_output_contract/fixture.rs"]
mod fixture;

const BODY: &[u8] = b"self-authored transport bytes for metadata validation";

#[test]
fn binding_accepts_later_grant_issuance_but_never_a_foreign_or_premature_permit() {
    let (job, permit, mut request, _) = fixture::parts();
    request.binding.issued_at += 100;
    let make = |permit: &p::WorkerPermit, request: &p::DesktopActionRequest| {
        p::DesktopExchangeBinding::from_request("installation:output".into(), &job, permit, request)
    };
    make(&permit, &request).unwrap();
    let mut changed = permit.clone();
    changed.issued_at = request.binding.issued_at + 1;
    assert!(make(&changed, &request).is_err());
    let mut changed = permit.clone();
    changed.owner_ref.0 = "owner:foreign".into();
    assert!(make(&changed, &request).is_err());
    let mut changed = permit.clone();
    changed.affected_resources.clear();
    assert!(make(&changed, &request).is_err());
    request.deadline = request.binding.issued_at;
    assert!(make(&permit, &request).is_err());
}

fn material() -> p::WorkerArtifactBinding {
    let (request, _, transfer) = fixture::request();
    let header = fixture::header(&transfer, BODY);
    let b = &transfer.binding;
    let mut manifest = p::WorkerResultManifest {
        schema_version: p::SchemaVersion(1),
        manifest_digest: p::SchemaDigest(String::new()),
        binding: p::WorkerResultBinding {
            schema_version: p::SchemaVersion(1),
            installation_ref: b.installation_ref.clone(),
            job_id: b.job_id.clone(),
            owner_ref: b.owner_ref.clone(),
            action_ref: b.action_ref.clone(),
            workspace_scope: b.workspace_scope.clone(),
            plan_digest: b.plan_digest.clone(),
            normalized_action_digest: Some(b.normalized_action_digest.clone()),
            permit_ref: b.permit_ref.clone(),
            permit_digest: b.permit_digest.clone(),
            authority_epoch: b.authority_epoch,
            nonce: b.nonce.clone(),
            receipt_ref: Some("receipt:output".into()),
            output_budget_bytes: transfer.output_budget_bytes,
        },
        result_ref: Some("result:output".into()),
        status: p::WorkerResultStatus::Available,
        outcome: Some(p::WorkerOutcome::Completed),
        bytes: BODY.len() as u64,
        content_digest: Some(header.content_digest.clone()),
        encoding: p::WorkerResultEncoding::Binary,
        truncated: false,
        produced_at: request.binding.issued_at,
        evidence_refs: vec![],
    };
    manifest.manifest_digest = manifest.expected_digest().unwrap();
    let descriptor = p::DesktopOutputDescriptor {
        schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
        binding: transfer.binding,
        manifest_digest: manifest.manifest_digest.clone(),
        content_digest: header.content_digest,
        bytes: header.bytes,
        media_type: header.media_type,
        width: header.width,
        height: header.height,
    };
    p::WorkerArtifactBinding {
        schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
        manifest,
        descriptor,
    }
}

#[test]
fn transfer_requires_the_exact_original_request_and_screenshot_intent() {
    let (request, intent, transfer) = fixture::request();
    transfer.binding.validate_request(&request).unwrap();
    transfer.validate_intent(&intent).unwrap();
    let original = serde_json::to_vec(&request).unwrap();
    assert_eq!(p::DESKTOP_SESSION_SCHEMA_VERSION, p::SchemaVersion(1));
    assert_eq!(p::WORKER_RESULT_SCHEMA_VERSION, p::SchemaVersion(1));
    assert_eq!(
        serde_json::from_slice::<p::DesktopActionRequest>(&original).unwrap(),
        request
    );
    for (field, replacement) in [("job_id", "job:foreign"), ("permit_ref", "permit:foreign")] {
        let mut value = serde_json::to_value(&request).unwrap();
        value[field] = replacement.into();
        let changed = serde_json::from_value(value).unwrap();
        assert!(
            transfer.binding.validate_request(&changed).is_err(),
            "{field}"
        );
    }
    for field in [
        "owner_ref",
        "action_ref",
        "grant_ref",
        "os_session_ref",
        "nonce",
    ] {
        let mut value = serde_json::to_value(&request).unwrap();
        value["binding"][field] = "foreign:identity".into();
        let changed = serde_json::from_value(value).unwrap();
        assert!(
            transfer.binding.validate_request(&changed).is_err(),
            "{field}"
        );
    }
    for field in ["authority_epoch", "session_generation"] {
        let mut value = serde_json::to_value(&request).unwrap();
        value["binding"][field] = 2.into();
        assert!(transfer
            .binding
            .validate_request(&serde_json::from_value(value).unwrap())
            .is_err());
    }
    let mut changed = request.clone();
    changed.deadline += 1;
    assert!(transfer.binding.validate_request(&changed).is_err());
    changed = request.clone();
    changed.normalized_action_digest = p::sha256_content_digest(b"another-plan");
    assert!(transfer.binding.validate_request(&changed).is_err());
    let mut changed = intent.clone();
    changed.scope = p::Scope("workspace:foreign".into());
    assert!(transfer.validate_intent(&changed).is_err());
    let p::ActionParameters::Computer(spec) = &mut changed.parameters else {
        panic!()
    };
    spec.operation = p::ComputerOperation::Move { x: 0, y: 0 };
    assert!(p::desktop_output_dimensions(&changed).is_err());
    assert_eq!(serde_json::to_vec(&request).unwrap(), original);
}

#[test]
fn screenshot_bounds_and_byte_budget_are_independent_and_bounded() {
    let (_, intent, transfer) = fixture::request();
    assert!(p::DesktopOutputTransfer::new(transfer.binding.clone(), &intent, 0).is_err());
    let max = p::DesktopOutputTransfer::new(transfer.binding.clone(), &intent, u64::MAX).unwrap();
    assert_eq!(max.output_budget_bytes, p::DESKTOP_OUTPUT_MAX_BYTES);
    for (width, height) in [(0, 1), (1, 0), (4097, 4096), (u32::MAX, u32::MAX)] {
        let mut changed = transfer.clone();
        changed.width = width;
        changed.height = height;
        assert!(changed.validate().is_err());
    }
    let mut changed = transfer.clone();
    changed.transfer_ref = "transfer:foreign".into();
    assert!(changed.validate().is_err());
    let mut changed = intent.clone();
    let p::ActionParameters::Computer(spec) = &mut changed.parameters else {
        panic!()
    };
    spec.bounds.min_x = i32::MIN;
    spec.bounds.max_x_exclusive = i32::MAX;
    assert!(p::desktop_output_dimensions(&changed).is_err());
    let header = fixture::header(&transfer, BODY);
    for (field, value) in [
        ("bytes", serde_json::json!(4097)),
        ("width", serde_json::json!(3)),
        ("media_type", serde_json::json!("text/plain")),
    ] {
        let mut changed = serde_json::to_value(&header).unwrap();
        changed[field] = value;
        assert!(serde_json::from_value::<p::DesktopOutputHeader>(changed)
            .unwrap()
            .validate_for(&transfer)
            .is_err());
    }
}

#[test]
fn chunks_reject_cross_transfer_offsets_overflow_empty_and_wrong_original_digest() {
    let (_, _, transfer) = fixture::request();
    let header = fixture::header(&transfer, BODY);
    let split = BODY.len() / 2;
    let first = fixture::chunk(&header, &BODY[..split], 0);
    let last = fixture::chunk(&header, &BODY[split..], split as u64);
    first.validate_for(&header, 0, &BODY[..split]).unwrap();
    last.validate_for(&header, first.next_offset, &BODY[split..])
        .unwrap();
    assert!(!first.finished);
    assert!(last.finished);
    assert!(last.validate_for(&header, 0, &BODY[split..]).is_err());
    assert!(first
        .validate_for(&header, first.next_offset, &BODY[..split])
        .is_err());
    for (field, value) in [
        ("offset", serde_json::json!(u64::MAX)),
        ("bytes", serde_json::json!(0)),
        ("bytes", serde_json::json!(32769)),
        ("next_offset", serde_json::json!(u64::MAX)),
        ("finished", serde_json::json!(true)),
        ("transfer_ref", serde_json::json!("transfer:other")),
        (
            "chunk_digest",
            serde_json::json!(p::sha256_content_digest(b"changed")),
        ),
        (
            "binding_digest",
            serde_json::json!(p::sha256_content_digest(b"foreign-binding")),
        ),
    ] {
        let mut changed = serde_json::to_value(&first).unwrap();
        changed[field] = value;
        assert!(
            serde_json::from_value::<p::DesktopOutputChunkHeader>(changed)
                .unwrap()
                .validate_for(&header, 0, &BODY[..split])
                .is_err(),
            "{field}"
        );
    }
}

#[test]
fn finish_cannot_turn_unknown_foreign_or_late_receipts_into_completion() {
    let (request, _, transfer) = fixture::request();
    let header = fixture::header(&transfer, BODY);
    let finish = fixture::finish(&request, &header);
    finish.validate_for(&header, &request).unwrap();
    for outcome in [
        p::DesktopActionOutcome::Unknown,
        p::DesktopActionOutcome::Failed,
        p::DesktopActionOutcome::Cancelled,
    ] {
        let mut changed = finish.clone();
        changed.receipt.outcome = outcome;
        assert!(changed.validate_for(&header, &request).is_err());
    }
    for time in [request.binding.issued_at - 1, request.deadline + 1] {
        let mut changed = finish.clone();
        changed.receipt.observed_at = time;
        assert!(changed.validate_for(&header, &request).is_err());
    }
    for field in ["header_digest", "content_digest", "binding_digest"] {
        let mut changed = serde_json::to_value(&finish).unwrap();
        changed[field] = serde_json::to_value(p::sha256_content_digest(b"replacement")).unwrap();
        assert!(serde_json::from_value::<p::DesktopOutputFinish>(changed)
            .unwrap()
            .validate_for(&header, &request)
            .is_err());
    }
    let mut changed = finish.clone();
    changed.receipt.result_ref = Some("result:unrelated".into());
    assert!(changed.validate_for(&header, &request).is_err());
}

#[test]
fn descriptor_requires_original_available_binary_manifest_and_stable_material_identity() {
    let original = material();
    original.validate().unwrap();
    assert_eq!(
        original.object_id().unwrap(),
        original.clone().object_id().unwrap()
    );
    let digest = original.descriptor.digest().unwrap();
    for case in 0..6 {
        let mut changed = original.clone();
        match case {
            0 => changed.manifest.binding.owner_ref.0 = "owner:foreign".into(),
            1 => changed.manifest.encoding = p::WorkerResultEncoding::Utf8,
            2 => changed.manifest.outcome = Some(p::WorkerOutcome::Unknown),
            3 => changed.manifest.produced_at = changed.descriptor.binding.deadline + 1,
            4 => changed.manifest.binding.receipt_ref = None,
            _ => changed.manifest.content_digest = Some(p::sha256_content_digest(b"replacement")),
        }
        // A newly self-consistent manifest hash cannot conceal other mismatches.
        changed.manifest.manifest_digest = changed.manifest.expected_digest().unwrap();
        changed.descriptor.manifest_digest = changed.manifest.manifest_digest.clone();
        assert!(changed.validate().is_err(), "case {case}");
    }
    let mut changed = original.clone();
    changed.descriptor.binding.grant_ref = "grant:another".into();
    assert_ne!(changed.descriptor.digest().unwrap(), digest);
    assert_ne!(changed.object_id().unwrap(), original.object_id().unwrap());
}

#[test]
fn every_output_object_rejects_unknown_fields_and_legacy_versions_remain_closed() {
    let (request, _, transfer) = fixture::request();
    let header = fixture::header(&transfer, BODY);
    macro_rules! closed {
        ($value:expr, $kind:ty) => {{
            let value = $value;
            let encoded = serde_json::to_vec(&value).unwrap();
            let decoded: $kind = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
            let mut changed = serde_json::to_value(&value).unwrap();
            changed["unexpected"] = true.into();
            assert!(serde_json::from_value::<$kind>(changed).is_err());
        }};
    }
    closed!(transfer.binding.clone(), p::DesktopExchangeBinding);
    closed!(transfer.clone(), p::DesktopOutputTransfer);
    closed!(header.clone(), p::DesktopOutputHeader);
    closed!(
        fixture::chunk(&header, BODY, 0),
        p::DesktopOutputChunkHeader
    );
    closed!(fixture::finish(&request, &header), p::DesktopOutputFinish);
    closed!(material().descriptor, p::DesktopOutputDescriptor);
    closed!(material(), p::WorkerArtifactBinding);
    for version in [0, 1, 3, u32::MAX] {
        let mut changed = transfer.clone();
        changed.schema_version = p::SchemaVersion(version);
        assert!(changed.validate().is_err());
        let mut changed = material();
        changed.schema_version = p::SchemaVersion(version);
        assert!(changed.validate().is_err());
    }
    let mut changed = serde_json::to_value(request).unwrap();
    changed["output"] = true.into();
    assert!(serde_json::from_value::<p::DesktopActionRequest>(changed).is_err());
}
