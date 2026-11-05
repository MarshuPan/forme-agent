//! Structural contracts only: these fixtures do not authenticate evidence.
use forme_protocol as p;

fn hash(label: &str) -> p::SchemaDigest {
    p::sha256_content_digest(label.as_bytes())
}

fn report() -> p::LegacyReleaseReview {
    let binding = p::LegacyReleaseReviewBinding {
        schema_version: p::SchemaVersion(1),
        review_id: "review:contract".into(),
        install_ref: "install:contract".into(),
        owner: p::OwnerPrincipalRef("owner:contract".into()),
        original_operation_id: "upgrade:contract".into(),
        original_command_digest: hash("command"),
        original_release_ref: "1.0.0".into(),
        original_release_digest: hash("release"),
        config_revision: 2,
        config_digest: hash("config"),
        journal_seq: 10,
        journal_head: hash("head"),
        expires_at: 100_000,
    };
    let evidence = p::LegacyReleaseEvidenceKind::ALL
        .into_iter()
        .map(|kind| p::LegacyReleaseEvidence {
            schema_version: p::SchemaVersion(1),
            kind,
            artifact_digest: hash("artifact"),
            binding_digest: binding.digest().unwrap(),
            observation_digest: hash("observation"),
        })
        .collect();
    p::LegacyReleaseReview {
        schema_version: p::SchemaVersion(1),
        binding,
        evidence,
        status: p::LegacyReleaseReviewStatus::Verified,
        issues: vec![],
    }
}

fn request(review: &p::LegacyReleaseReview) -> p::LegacyReleaseRecoveryRequest {
    p::LegacyReleaseRecoveryRequest {
        schema_version: p::SchemaVersion(1),
        operation_id: "decision:contract".into(),
        idempotency_key: "key:decision".into(),
        expected_revision: 10,
        owner: review.binding.owner.clone(),
        review_id: review.binding.review_id.clone(),
        review_digest: review.digest().unwrap(),
        requested_deadline: 90_000,
    }
}

/// Shared wire golden with frontend/src/tests/legacy-release-recovery.test.ts.
/// Both languages must match Rust's declared struct order, not sorted JSON.
#[test]
fn legacy_wire_digest_golden_matches_the_browser_report_and_request_binding() {
    let review = report();
    assert_eq!(
        review.binding.digest().unwrap().0,
        "sha256:5330285943e7ea4964a0848cdf8a1dd805efa4e0767c92db0422abd0259af665"
    );
    assert_eq!(
        review.digest().unwrap().0,
        "sha256:9be2f960600222cad01a2387f83bc643d12f31e1ae184526b90ed5d514f3a4bc"
    );
    assert_eq!(
        request(&review).digest().unwrap().0,
        "sha256:caf04d5763c2ccc8aad79a17ed39ca3bedf2cb5754937e4dbb495da32a456b1d"
    );
}

#[test]
fn legacy_report_requires_complete_ordered_same_binding_evidence() {
    let valid = report();
    valid.validate().unwrap();
    for mode in 0..8 {
        let mut changed = valid.clone();
        match mode {
            0 => {
                changed.evidence.remove(0);
            }
            1 => changed.evidence.swap(0, 1),
            2 => changed.evidence[1] = changed.evidence[0].clone(),
            3 => changed.evidence[0].binding_digest = hash("foreign-binding"),
            4 => changed.evidence[0].artifact_digest = p::SchemaDigest("unhashed".into()),
            5 => changed.evidence[0].schema_version = p::SchemaVersion(2),
            6 => changed.issues.push("issue:unverified-source".into()),
            _ => changed.evidence.push(changed.evidence[0].clone()),
        }
        assert!(changed.validate().is_err(), "mode {mode}");
    }
}

#[test]
fn legacy_rejected_and_unverifiable_reports_cannot_omit_or_repeat_issues() {
    for status in [
        p::LegacyReleaseReviewStatus::Rejected,
        p::LegacyReleaseReviewStatus::Unverifiable,
    ] {
        let mut review = report();
        review.status = status;
        review.evidence.clear();
        assert!(review.validate().is_err());
        review.issues = vec!["issue:missing-archive".into()];
        review.validate().unwrap();
        review.issues.push("issue:missing-archive".into());
        assert!(review.validate().is_err());
    }
}

#[test]
fn legacy_binding_digest_covers_every_source_identity_and_checkpoint() {
    let binding = report().binding;
    let original = binding.digest().unwrap();
    for mode in 0..13 {
        let mut changed = binding.clone();
        match mode {
            0 => changed.review_id = "review:other".into(),
            1 => changed.install_ref = "install:other".into(),
            2 => changed.owner = p::OwnerPrincipalRef("owner:other".into()),
            3 => changed.original_operation_id = "upgrade:other".into(),
            4 => changed.original_command_digest = hash("other-command"),
            5 => changed.original_release_ref = "1.0.1".into(),
            6 => changed.original_release_digest = hash("other-release"),
            7 => changed.config_revision += 1,
            8 => changed.config_digest = hash("other-config"),
            9 => changed.journal_seq += 1,
            10 => changed.journal_head = hash("other-head"),
            11 => changed.expires_at += 1,
            _ => {
                changed.schema_version = p::SchemaVersion(2);
                assert!(changed.digest().is_err());
                continue;
            }
        }
        assert_ne!(changed.digest().unwrap(), original, "mode {mode}");
    }
}

#[test]
fn legacy_commands_are_closed_and_verification_has_independent_authorization_digest() {
    let review = report();
    let request = request(&review);
    let digest = request.digest().unwrap();
    let verification = p::LegacyReleaseRecoveryVerification {
        schema_version: p::SchemaVersion(1),
        operation_id: request.operation_id.clone(),
        request_digest: digest.clone(),
        expected_revision: request.expected_revision + 1,
        owner: request.owner.clone(),
        requested_deadline: request.requested_deadline + 1,
    };
    assert_ne!(verification.digest().unwrap(), digest);
    let mut changed = verification.clone();
    changed.requested_deadline += 1;
    assert_ne!(changed.digest().unwrap(), verification.digest().unwrap());
    for key in [
        "program",
        "path",
        "public_key",
        "verified",
        "evidence",
        "review",
    ] {
        let mut wire = serde_json::to_value(&request).unwrap();
        wire[key] = serde_json::json!(true);
        assert!(serde_json::from_value::<p::LegacyReleaseRecoveryRequest>(wire).is_err());
        let mut wire = serde_json::to_value(&verification).unwrap();
        wire[key] = serde_json::json!(true);
        assert!(serde_json::from_value::<p::LegacyReleaseRecoveryVerification>(wire).is_err());
    }
    let signed = p::SignedLegacyReleaseReview {
        schema_version: p::SchemaVersion(1),
        review,
        signature: vec![0; 64],
    };
    signed.validate().unwrap(); // Length/shape is deliberately not authenticity.
    let mut wire = serde_json::to_value(&signed).unwrap();
    wire["public_key"] = serde_json::json!([]);
    assert!(serde_json::from_value::<p::SignedLegacyReleaseReview>(wire).is_err());
}

#[test]
fn legacy_views_and_lists_preserve_owner_revision_order_and_receipt_boundaries() {
    let request = request(&report());
    let mut view = p::LegacyReleaseRecoveryView {
        schema_version: p::SchemaVersion(1),
        request_digest: request.digest().unwrap(),
        revision: 11,
        stage: p::LegacyReleaseRecoveryStage::Intent,
        review_digest: request.review_digest.clone(),
        receipt_digest: None,
        request,
    };
    view.validate().unwrap();
    view.stage = p::LegacyReleaseRecoveryStage::Committed;
    assert!(view.validate().is_err());
    view.receipt_digest = Some(hash("receipt"));
    view.validate().unwrap();
    let mut list = p::LegacyReleaseRecoveryList {
        schema_version: p::SchemaVersion(1),
        product_revision: 11,
        owner: view.request.owner.clone(),
        operations: vec![view.clone()],
    };
    list.validate().unwrap();
    list.owner = p::OwnerPrincipalRef("owner:foreign".into());
    assert!(list.validate().is_err());
    list.owner = view.request.owner.clone();
    list.operations.push(view);
    assert!(list.validate().is_err());
}

#[test]
fn recovery_capabilities_accept_only_exact_supported_reader_updater_pairs() {
    for reader in 0..=4 {
        for updater in 0..=4 {
            let capabilities = p::ProductReleaseRecoveryCapabilities {
                schema_version: p::SchemaVersion(1),
                journal_read_max: reader,
                updater_version: updater,
            };
            assert_eq!(
                capabilities.validate().is_ok(),
                matches!((reader, updater), (2, 2) | (3, 3))
            );
        }
    }
}
