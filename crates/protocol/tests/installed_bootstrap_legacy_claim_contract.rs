use forme_protocol as p;

fn digest(value: &str) -> p::SchemaDigest {
    p::canonical_digest(&("bootstrap-fixture", value)).unwrap()
}

fn candidate(
    kind: p::BootstrapCandidateKind,
    status: p::BootstrapCandidateStatus,
) -> p::BootstrapCandidateViewV2 {
    let (owner_source, owner_ref, owner, store, schema) = match kind {
        p::BootstrapCandidateKind::NewBrainTarget => (
            p::BootstrapOwnerSource::GeneratedForNewBrain,
            None,
            None,
            None,
            None,
        ),
        p::BootstrapCandidateKind::ExistingBrain | p::BootstrapCandidateKind::VerifiedBackup => (
            p::BootstrapOwnerSource::VerifiedFromCandidate,
            Some(p::OwnerPrincipalRef("owner:fixture".into())),
            Some(
                p::canonical_digest(&(
                    "forme-bootstrap-owner-binding-v2",
                    &p::OwnerPrincipalRef("owner:fixture".into()),
                ))
                .unwrap(),
            ),
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
        owner_ref,
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
fn candidate_owner_reference_and_binding_are_tamper_evident() {
    let mut item = candidate(
        p::BootstrapCandidateKind::ExistingBrain,
        p::BootstrapCandidateStatus::Available,
    );
    item.owner_binding_digest = Some(digest("wrong-owner-binding"));
    item.candidate_digest = item.expected_digest("installation:fixture").unwrap();
    assert!(item.validate("installation:fixture").is_err());

    let mut item = candidate(
        p::BootstrapCandidateKind::ExistingBrain,
        p::BootstrapCandidateStatus::Available,
    );
    item.owner_ref = Some(p::OwnerPrincipalRef("owner:changed".into()));
    item.candidate_digest = item.expected_digest("installation:fixture").unwrap();
    assert!(item.validate("installation:fixture").is_err());
}

fn claim_receipt(outcome: p::BootstrapMigrationOutcome) -> p::BootstrapClaimReceiptV2 {
    let owner_ref = p::OwnerPrincipalRef("owner:fixture".into());
    p::BootstrapClaimReceiptV2 {
        schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
        install_ref: "installation:fixture".into(),
        operation_id: "claim:fixture".into(),
        candidate_ref: "candidate:existing".into(),
        candidate_kind: p::BootstrapCandidateKind::ExistingBrain,
        candidate_digest: digest("candidate"),
        owner_source: p::BootstrapOwnerSource::VerifiedFromCandidate,
        owner_binding_digest: p::canonical_digest(&(
            "forme-bootstrap-owner-binding-v2",
            &owner_ref,
        ))
        .unwrap(),
        owner_ref,
        account_commitment: digest("account"),
        target_binding_digest: digest("target"),
        migration_outcome: outcome,
        expires_at: 2_000_000,
    }
}

fn claim_view(phase: p::BootstrapClaimPhaseV2) -> p::BootstrapClaimViewV2 {
    let has_candidate = !matches!(phase, p::BootstrapClaimPhaseV2::Observed);
    let has_receipt = matches!(
        phase,
        p::BootstrapClaimPhaseV2::Committed | p::BootstrapClaimPhaseV2::Activated
    );
    let has_activation = phase == p::BootstrapClaimPhaseV2::Activated;
    let receipt = has_receipt.then(|| {
        claim_receipt(if has_activation {
            p::BootstrapMigrationOutcome::Recovered
        } else {
            p::BootstrapMigrationOutcome::Prepared
        })
    });
    let activation = has_activation.then(|| {
        let receipt = receipt.as_ref().unwrap();
        p::BootstrapClaimActivationV2 {
            schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
            operation_id: receipt.operation_id.clone(),
            receipt_digest: receipt.digest().unwrap(),
            active_data_binding_digest: receipt.target_binding_digest.clone(),
        }
    });
    p::BootstrapClaimViewV2 {
        schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
        install_ref: "installation:fixture".into(),
        operation_id: "claim:fixture".into(),
        phase,
        candidate_ref: has_candidate.then_some("candidate:existing".into()),
        candidate_digest: has_candidate.then_some(digest("candidate")),
        verification_epoch: has_candidate.then_some(7),
        receipt,
        active_data_binding_digest: activation
            .as_ref()
            .map(|item| item.active_data_binding_digest.clone()),
        activation,
        issue_ref: matches!(
            phase,
            p::BootstrapClaimPhaseV2::Aborted | p::BootstrapClaimPhaseV2::RecoveryRequired
        )
        .then_some("issue:fixture".into()),
    }
}

#[test]
fn claim_view_phase_matrix_accepts_only_consistent_bindings() {
    for phase in [
        p::BootstrapClaimPhaseV2::Observed,
        p::BootstrapClaimPhaseV2::Selected,
        p::BootstrapClaimPhaseV2::DataPrepared,
        p::BootstrapClaimPhaseV2::Committed,
        p::BootstrapClaimPhaseV2::Activated,
        p::BootstrapClaimPhaseV2::Aborted,
        p::BootstrapClaimPhaseV2::RecoveryRequired,
    ] {
        claim_view(phase).validate().unwrap();
    }

    let mut invalid = claim_view(p::BootstrapClaimPhaseV2::Selected);
    invalid.receipt = Some(claim_receipt(p::BootstrapMigrationOutcome::Prepared));
    assert!(invalid.validate().is_err());

    let mut invalid = claim_view(p::BootstrapClaimPhaseV2::DataPrepared);
    invalid.verification_epoch = None;
    assert!(invalid.validate().is_err());

    let mut invalid = claim_view(p::BootstrapClaimPhaseV2::RecoveryRequired);
    invalid.issue_ref = None;
    assert!(invalid.validate().is_err());

    let mut invalid = claim_view(p::BootstrapClaimPhaseV2::Aborted);
    invalid.active_data_binding_digest = Some(digest("active"));
    assert!(invalid.validate().is_err());

    let mut invalid = claim_view(p::BootstrapClaimPhaseV2::Activated);
    invalid.activation = None;
    assert!(invalid.validate().is_err());

    let mut invalid = claim_view(p::BootstrapClaimPhaseV2::Committed);
    invalid.candidate_digest = Some(digest("other-candidate"));
    assert!(invalid.validate().is_err());
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
fn claim_commit_and_activation_have_separate_boundaries() {
    let mut receipt = p::BootstrapClaimReceiptV2 {
        schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
        install_ref: "installation:fixture".into(),
        operation_id: "claim:fixture".into(),
        candidate_ref: "candidate:existing".into(),
        candidate_kind: p::BootstrapCandidateKind::ExistingBrain,
        candidate_digest: digest("candidate"),
        owner_source: p::BootstrapOwnerSource::VerifiedFromCandidate,
        owner_ref: p::OwnerPrincipalRef("owner:fixture".into()),
        owner_binding_digest: p::canonical_digest(&(
            "forme-bootstrap-owner-binding-v2",
            &p::OwnerPrincipalRef("owner:fixture".into()),
        ))
        .unwrap(),
        account_commitment: digest("account"),
        target_binding_digest: digest("target"),
        migration_outcome: p::BootstrapMigrationOutcome::Prepared,
        expires_at: 2_000_000,
    };
    let prepared = p::BootstrapClaimDataPreparedV2 {
        schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
        operation_id: receipt.operation_id.clone(),
        candidate_ref: receipt.candidate_ref.clone(),
        candidate_digest: receipt.candidate_digest.clone(),
        verification_epoch: 7,
        target_binding_digest: receipt.target_binding_digest.clone(),
        migration_outcome: receipt.migration_outcome,
        worker_receipt_digest: digest("worker-receipt"),
    };
    let commit = p::BootstrapClaimCommitV2 {
        schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
        receipt: receipt.clone(),
    };
    assert!(commit.validate_against_prepared(&prepared).is_ok());

    let activation = p::BootstrapClaimActivationV2 {
        schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
        operation_id: receipt.operation_id.clone(),
        receipt_digest: receipt.digest().unwrap(),
        active_data_binding_digest: receipt.target_binding_digest.clone(),
    };
    assert!(activation.validate_against_commit(&commit).is_err());

    receipt.migration_outcome = p::BootstrapMigrationOutcome::Recovered;
    let activation = p::BootstrapClaimActivationV2 {
        schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
        operation_id: receipt.operation_id.clone(),
        receipt_digest: receipt.digest().unwrap(),
        active_data_binding_digest: receipt.target_binding_digest.clone(),
    };
    let recovered_commit = p::BootstrapClaimCommitV2 {
        schema_version: p::BOOTSTRAP_CANDIDATE_SCHEMA_VERSION,
        receipt: receipt.clone(),
    };
    let recovered_prepared = p::BootstrapClaimDataPreparedV2 {
        migration_outcome: p::BootstrapMigrationOutcome::Recovered,
        ..prepared
    };
    assert!(recovered_commit
        .validate_against_prepared(&recovered_prepared)
        .is_ok());
    assert!(activation
        .validate_against_commit(&recovered_commit)
        .is_ok());

    let mut wrong_receipt_binding = activation.clone();
    wrong_receipt_binding.receipt_digest = digest("other-receipt");
    assert!(wrong_receipt_binding
        .validate_against_commit(&recovered_commit)
        .is_err());

    let mut wrong_active_binding = activation;
    wrong_active_binding.active_data_binding_digest = digest("other-target");
    assert!(wrong_active_binding
        .validate_against_commit(&recovered_commit)
        .is_err());
}
