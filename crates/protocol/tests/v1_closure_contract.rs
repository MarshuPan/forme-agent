use forme_protocol as p;
use std::str::FromStr;

const EMPTY_SHA256: &str =
    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn charter() -> p::WorkspaceCharterRecord {
    let mut charter = p::WorkspaceCharterRecord {
        schema_version: p::V1_CLOSURE_SCHEMA_VERSION,
        workspace: p::WorkspaceRef("workspace:forme".into()),
        version: 1,
        goals: vec![p::GoalRef("goal:ship".into())],
        constraints: vec![p::Constraint("constraint:owner-only".into())],
        prohibitions: vec![p::Constraint("prohibition:secrets-in-receipt".into())],
        done_contract: Some(p::DoneContractRef("done:closure".into())),
        review_cadence: Some(p::DurationMs(3_600_000)),
        actor: p::Actor::Owner,
        digest: p::SchemaDigest(String::new()),
    };
    charter.refresh_digest().unwrap();
    charter
}

fn receipt(operation: p::DataLifecycleOperation) -> p::DataLifecycleReceipt {
    p::DataLifecycleReceipt {
        schema_version: p::V1_CLOSURE_SCHEMA_VERSION,
        aggregate: p::RunId("run:lifecycle".into()),
        operation,
        scope: p::Scope("workspace:forme".into()),
        subject_digest: p::SchemaDigest(EMPTY_SHA256.into()),
        cleaned_projections: vec![p::ProjectionRef("projection:memory".into())],
        destroyed_key_digests: vec![p::SchemaDigest(EMPTY_SHA256.into())],
        remote_disposition: p::RemoteDeletionDisposition::NotApplicable,
        evidence: vec![p::EvidenceRef("evidence:lifecycle".into())],
    }
}

#[test]
fn closure_taxonomy_preserves_the_exact_m5_prefix_and_adds_only_two_events() {
    assert_eq!(p::M5_EVENT_KIND_COUNT, 97);
    assert_eq!(p::EventKind::ALL.len(), 99);
    assert_eq!(p::EventKind::ALL.len(), p::M5_EVENT_KIND_COUNT + 2);
    assert_eq!(
        p::EventKind::ALL[p::M5_EVENT_KIND_COUNT],
        p::EventKind::WorkspaceCharterChanged
    );
    assert_eq!(
        &p::EventKind::ALL[97..],
        &[
            p::EventKind::WorkspaceCharterChanged,
            p::EventKind::DataLifecycleApplied,
        ]
    );
    assert_eq!(
        p::EventKind::from_str("WorkspaceCharterChanged").unwrap(),
        p::EventKind::WorkspaceCharterChanged
    );
    assert_eq!(
        p::EventKind::from_str("DataLifecycleApplied").unwrap(),
        p::EventKind::DataLifecycleApplied
    );
}

#[test]
fn charter_is_owner_authored_versioned_and_digest_bound() {
    let mut value = charter();
    value.validate().unwrap();

    let mut changed = value.clone();
    changed.goals.push(p::GoalRef("goal:observe".into()));
    assert!(changed.validate().is_err());

    changed = value.clone();
    changed.actor = p::Actor::Agent;
    assert!(changed.validate().is_err());

    changed = value.clone();
    changed.review_cadence = Some(p::DurationMs(0));
    changed.refresh_digest().unwrap();
    assert!(changed.validate().is_err());

    value.version = 2;
    assert!(value.validate().is_err());
}

#[test]
fn closure_payloads_validate_cas_and_round_trip() {
    let charter_payload = p::WorkspaceCharterChangedPayload {
        charter: charter(),
        expected_version: 0,
        committed_version: 1,
    };
    charter_payload.validate().unwrap();

    let lifecycle_payload = p::DataLifecycleAppliedPayload {
        receipt: receipt(p::DataLifecycleOperation::CryptoShred),
        expected_version: 0,
        committed_version: 1,
    };
    lifecycle_payload.validate().unwrap();

    let payloads = [
        p::EventPayload::WorkspaceCharterChanged(charter_payload),
        p::EventPayload::DataLifecycleApplied(lifecycle_payload),
    ];
    for payload in payloads {
        let encoded = serde_json::to_vec(&payload).unwrap();
        let decoded: p::EventPayload = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.kind(), payload.kind());
    }
}

#[test]
fn lifecycle_receipt_rejects_secret_endpoint_path_and_invalid_digest_markers() {
    let mut value = receipt(p::DataLifecycleOperation::Delete);
    value.aggregate = p::RunId("secret:run".into());
    assert!(value.validate().is_err());

    value = receipt(p::DataLifecycleOperation::Delete);
    value.cleaned_projections = vec![p::ProjectionRef("https://private.example".into())];
    assert!(value.validate().is_err());

    value = receipt(p::DataLifecycleOperation::Delete);
    value.subject_digest = p::SchemaDigest("sha256:not-a-digest".into());
    assert!(value.validate().is_err());

    value = receipt(p::DataLifecycleOperation::CryptoShred);
    value.destroyed_key_digests.clear();
    assert!(value.validate().is_err());
}

#[test]
fn lifecycle_and_charter_payloads_reject_non_monotonic_versions() {
    let mut charter_payload = p::WorkspaceCharterChangedPayload {
        charter: charter(),
        expected_version: 1,
        committed_version: 2,
    };
    assert!(charter_payload.validate().is_err());

    charter_payload.expected_version = 0;
    charter_payload.committed_version = 2;
    assert!(charter_payload.validate().is_err());

    let lifecycle_payload = p::DataLifecycleAppliedPayload {
        receipt: receipt(p::DataLifecycleOperation::Retain),
        expected_version: 2,
        committed_version: 2,
    };
    assert!(lifecycle_payload.validate().is_err());
}
