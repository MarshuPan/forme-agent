use forme_protocol as p;

fn digest(value: &str) -> p::SchemaDigest {
    p::canonical_digest(&("bootstrap-fixture", value)).unwrap()
}

fn candidate(
    kind: p::BootstrapCandidateKind,
    status: p::BootstrapCandidateStatus,
) -> p::BootstrapCandidateViewV2 {
    let (owner_source, owner, store, schema) = match kind {
        p::BootstrapCandidateKind::NewBrainTarget => (
            p::BootstrapOwnerSource::GeneratedForNewBrain,
            None,
            None,
            None,
        ),
        p::BootstrapCandidateKind::ExistingBrain | p::BootstrapCandidateKind::VerifiedBackup => (
            p::BootstrapOwnerSource::VerifiedFromCandidate,
            Some(digest("owner")),
            Some(digest("store")),
            Some(digest("schema")),
        ),
    };
    let mut result = p::BootstrapCandidateViewV2 {
        schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
        candidate_ref: match kind {
            p::BootstrapCandidateKind::NewBrainTarget => "candidate:new".into(),
            p::BootstrapCandidateKind::ExistingBrain => "candidate:existing".into(),
            p::BootstrapCandidateKind::VerifiedBackup => "candidate:backup".into(),
        },
        kind,
        status,
        candidate_digest: digest("placeholder"),
        verification_epoch: 7,
        owner_source,
        owner_binding_digest: owner,
        store_identity_digest: store,
        schema_digest: schema,
        protection_state: if status == p::BootstrapCandidateStatus::Available {
            p::BootstrapProtectionState::Protected
        } else {
            p::BootstrapProtectionState::NeedsMigration
        },
        source_observation_digest: digest("observation"),
        issue_ref: (status != p::BootstrapCandidateStatus::Available)
            .then_some("issue:fixture".into()),
    };
    result.candidate_digest = result.expected_digest("installation:fixture").unwrap();
    result
}

#[test]
fn three_candidate_kinds_round_trip_with_safe_views_only() {
    let candidates = vec![
        candidate(
            p::BootstrapCandidateKind::NewBrainTarget,
            p::BootstrapCandidateStatus::Available,
        ),
        candidate(
            p::BootstrapCandidateKind::ExistingBrain,
            p::BootstrapCandidateStatus::Available,
        ),
        candidate(
            p::BootstrapCandidateKind::VerifiedBackup,
            p::BootstrapCandidateStatus::Rejected,
        ),
    ];
    let mut catalog = p::BootstrapCandidateCatalogV2 {
        schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
        install_ref: "installation:fixture".into(),
        verification_epoch: 7,
        candidates,
        catalog_digest: digest("placeholder"),
    };
    catalog.catalog_digest = catalog.expected_digest().unwrap();
    catalog.validate().unwrap();

    let bytes = serde_json::to_vec(&catalog).unwrap();
    let decoded: p::BootstrapCandidateCatalogV2 = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded, catalog);
    assert!(decoded.candidate("candidate:existing").is_some());
    assert!(decoded.candidate("/absolute/path").is_none());
}

#[test]
fn candidate_digest_and_epoch_are_toc_tamper_evident() {
    let mut item = candidate(
        p::BootstrapCandidateKind::ExistingBrain,
        p::BootstrapCandidateStatus::Available,
    );
    item.validate("installation:fixture").unwrap();

    item.verification_epoch += 1;
    assert!(item.validate("installation:fixture").is_err());

    let mut selection = p::BootstrapCandidateSelectionV2 {
        schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
        session: p::BootstrapSessionBindingV2 {
            schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
            operation_id: "claim:fixture".into(),
            setup_digest: digest("setup"),
        },
        candidate_ref: "candidate:existing".into(),
        expected_candidate_digest: digest("candidate"),
    };
    selection.validate().unwrap();
    selection.expected_candidate_digest = digest("changed");
    assert!(selection.validate().is_ok());
}

#[test]
fn historical_candidate_cannot_claim_generated_or_unverified_identity() {
    let mut item = candidate(
        p::BootstrapCandidateKind::ExistingBrain,
        p::BootstrapCandidateStatus::Available,
    );
    item.owner_source = p::BootstrapOwnerSource::GeneratedForNewBrain;
    item.candidate_digest = item.expected_digest("installation:fixture").unwrap();
    assert!(item.validate("installation:fixture").is_err());

    let request = serde_json::json!({
        "schema_version": 2,
        "session": {
            "schema_version": 2,
            "operation_id": "claim:fixture",
            "setup_digest": digest("setup")
        },
        "candidate_ref": "candidate:existing",
        "expected_candidate_digest": digest("candidate"),
        "account_commitment": digest("account"),
        "owner": "owner:injected"
    });
    assert!(serde_json::from_value::<p::BootstrapClaimRequestV2>(request).is_err());
}

#[test]
fn claim_commit_cannot_publish_active_binding_before_recovery() {
    let receipt = p::BootstrapClaimReceiptV2 {
        schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
        install_ref: "installation:fixture".into(),
        operation_id: "claim:fixture".into(),
        candidate_ref: "candidate:existing".into(),
        candidate_kind: p::BootstrapCandidateKind::ExistingBrain,
        candidate_digest: digest("candidate"),
        owner_source: p::BootstrapOwnerSource::VerifiedFromCandidate,
        owner_binding_digest: digest("owner"),
        account_commitment: digest("account"),
        target_binding_digest: digest("target"),
        migration_outcome: p::BootstrapMigrationOutcome::Prepared,
        expires_at: 2_000_000,
    };
    let commit = p::BootstrapClaimCommitV2 {
        schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
        receipt,
        active_data_binding_digest: Some(digest("active")),
    };
    assert!(commit.validate().is_err());
}
