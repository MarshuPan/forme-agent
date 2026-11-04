//! Original isolated claim-journal fixtures. No data worker or native install.
use super::*;

fn digest(value: &str) -> p::SchemaDigest {
    p::canonical_digest(&("legacy-claim-journal-fixture", value)).unwrap()
}

fn setup() -> (
    Directory,
    BootstrapAuthorityConfig,
    BootstrapAuthority,
    p::BootstrapSessionBindingV2,
) {
    let (directory, config) = fixture();
    let authority = open(&config);
    let proof = authority
        .issue_proof(&admin(&directory), "proof:legacy-journal", NOW)
        .unwrap();
    let binding = session("claim:legacy-journal");
    reserve(&authority, &proof.proof, &binding, NOW).unwrap();
    (
        directory,
        config,
        authority,
        p::BootstrapSessionBindingV2 {
            schema_version: p::SchemaVersion(2),
            operation_id: binding.operation_id,
            setup_digest: binding.setup_digest,
        },
    )
}

fn catalog(kind: p::BootstrapCandidateKind) -> p::BootstrapCandidateCatalogV2 {
    let owner_ref = p::OwnerPrincipalRef("owner:legacy-fixture".into());
    let mut candidate = p::BootstrapCandidateViewV2 {
        schema_version: p::SchemaVersion(2),
        candidate_ref: "candidate:historical-fixture".into(),
        kind,
        status: p::BootstrapCandidateStatus::Available,
        candidate_digest: digest("pending-candidate-digest"),
        verification_epoch: 7,
        owner_source: p::BootstrapOwnerSource::VerifiedFromCandidate,
        owner_binding_digest: Some(
            p::canonical_digest(&("forme-bootstrap-owner-binding-v2", &owner_ref)).unwrap(),
        ),
        owner_ref: Some(owner_ref),
        store_identity_digest: Some(digest("store")),
        schema_digest: Some(digest("schema")),
        protection_state: p::BootstrapProtectionState::Protected,
        source_observation_digest: digest("source-observation"),
        issue_ref: None,
    };
    candidate.candidate_digest = candidate
        .expected_digest("installation:claim-fixture")
        .unwrap();
    let mut catalog = p::BootstrapCandidateCatalogV2 {
        schema_version: p::SchemaVersion(2),
        install_ref: "installation:claim-fixture".into(),
        verification_epoch: 7,
        candidates: vec![candidate],
        catalog_digest: digest("pending-catalog-digest"),
    };
    catalog.catalog_digest = catalog.expected_digest().unwrap();
    catalog.validate().unwrap();
    catalog
}

fn next_catalog(install: &str) -> p::BootstrapCandidateCatalogV2 {
    let mut result = catalog(p::BootstrapCandidateKind::ExistingBrain);
    result.install_ref = install.into();
    result.verification_epoch += 1;
    for item in &mut result.candidates {
        item.verification_epoch = result.verification_epoch;
        item.candidate_digest = item.expected_digest(install).unwrap();
    }
    result.catalog_digest = result.expected_digest().unwrap();
    result.validate().unwrap();
    result
}

fn append(authority: &BootstrapAuthority, fact: Fact) -> BootstrapResult<()> {
    authority.lock(NOW).unwrap().append(fact)
}

// Windows correctly denies an unrelated handle while the writer owns its
// exclusive range lock. Inspect through the writer's own handle instead.
fn bytes(authority: &BootstrapAuthority) -> Vec<u8> {
    authority.lock(NOW).unwrap().snapshot_for_test()
}

fn inventory(authority: &BootstrapAuthority, binding: &p::BootstrapSessionBindingV2) {
    append(
        authority,
        Fact::CandidateInventoryObserved {
            operation: binding.operation_id.clone(),
            catalog: catalog(p::BootstrapCandidateKind::ExistingBrain),
            at: NOW,
        },
    )
    .unwrap();
}

fn selection(binding: &p::BootstrapSessionBindingV2) -> Fact {
    let candidate = catalog(p::BootstrapCandidateKind::ExistingBrain).candidates[0].clone();
    Fact::CandidateSelectedV2 {
        selection: p::BootstrapCandidateSelectionV2 {
            schema_version: p::SchemaVersion(2),
            session: binding.clone(),
            candidate_ref: candidate.candidate_ref.clone(),
            expected_candidate_digest: candidate.candidate_digest.clone(),
        },
        candidate,
        at: NOW,
    }
}

fn prepared(binding: &p::BootstrapSessionBindingV2) -> p::BootstrapClaimDataPreparedV2 {
    let candidate = catalog(p::BootstrapCandidateKind::ExistingBrain).candidates[0].clone();
    p::BootstrapClaimDataPreparedV2 {
        schema_version: p::SchemaVersion(2),
        operation_id: binding.operation_id.clone(),
        candidate_ref: candidate.candidate_ref,
        candidate_digest: candidate.candidate_digest,
        verification_epoch: candidate.verification_epoch,
        target_binding_digest: digest("target-binding"),
        migration_outcome: p::BootstrapMigrationOutcome::Recovered,
        worker_receipt_digest: digest("worker-receipt"),
    }
}

fn committed(binding: &p::BootstrapSessionBindingV2) -> p::BootstrapClaimCommitV2 {
    let candidate = catalog(p::BootstrapCandidateKind::ExistingBrain).candidates[0].clone();
    p::BootstrapClaimCommitV2 {
        schema_version: p::SchemaVersion(2),
        receipt: p::BootstrapClaimReceiptV2 {
            schema_version: p::SchemaVersion(2),
            install_ref: "installation:claim-fixture".into(),
            operation_id: binding.operation_id.clone(),
            candidate_ref: candidate.candidate_ref,
            candidate_kind: candidate.kind,
            candidate_digest: candidate.candidate_digest,
            owner_source: candidate.owner_source,
            owner_ref: candidate.owner_ref.unwrap(),
            owner_binding_digest: candidate.owner_binding_digest.unwrap(),
            account_commitment: digest("account-commitment"),
            target_binding_digest: digest("target-binding"),
            migration_outcome: p::BootstrapMigrationOutcome::Recovered,
            expires_at: NOW + 5 * 60 * 1000,
        },
    }
}

fn activation(binding: &p::BootstrapSessionBindingV2) -> p::BootstrapClaimActivationV2 {
    p::BootstrapClaimActivationV2 {
        schema_version: p::SchemaVersion(2),
        operation_id: binding.operation_id.clone(),
        receipt_digest: committed(binding).receipt.digest().unwrap(),
        active_data_binding_digest: digest("target-binding"),
    }
}

fn view(
    authority: &BootstrapAuthority,
    binding: &p::BootstrapSessionBindingV2,
) -> p::BootstrapClaimViewV2 {
    authority
        .claim_state_v2(
            &authority.authenticate_service(WEB).unwrap(),
            p::BootstrapClaimQueryV2 {
                schema_version: p::SchemaVersion(2),
                session: binding.clone(),
            },
            NOW,
        )
        .unwrap()
}

fn prepare(authority: &BootstrapAuthority, binding: &p::BootstrapSessionBindingV2) {
    inventory(authority, binding);
    append(authority, selection(binding)).unwrap();
    append(
        authority,
        Fact::ClaimDataPrepared {
            prepared: prepared(binding),
            at: NOW,
        },
    )
    .unwrap();
}

fn abort(binding: &p::BootstrapSessionBindingV2) -> Fact {
    let candidate = catalog(p::BootstrapCandidateKind::ExistingBrain).candidates[0].clone();
    Fact::ClaimAbortedV2 {
        operation: binding.operation_id.clone(),
        candidate_ref: candidate.candidate_ref,
        candidate_digest: candidate.candidate_digest,
        issue_ref: "issue:owner-aborted-before-data".into(),
        at: NOW,
    }
}

fn assert_rejected_without_write(
    authority: &BootstrapAuthority,
    config: &BootstrapAuthorityConfig,
    fact: Fact,
) {
    let before = bytes(authority);
    let previous = authority.lock(NOW).unwrap().state.last_at;
    assert_eq!(
        append(authority, fact).unwrap_err().0,
        Code::RecoveryRequired
    );
    assert_eq!(bytes(authority), before);
    assert_eq!(authority.lock(NOW).unwrap().state.last_at, previous);
    assert_eq!(
        std::fs::read_dir(&config.new_brain_directory)
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn v2_claim_preserves_v1_prefix_and_separates_commit_from_activation() {
    let (_directory, config, authority, binding) = setup();
    let prefix = bytes(&authority);
    let key = authority
        .lock(NOW)
        .unwrap()
        .state
        .binding
        .as_ref()
        .unwrap()
        .1
        .clone();
    drop(authority);
    drop(Journal::open_v1(&config.journal_path, &config.install_ref, key.clone()).unwrap());
    let authority = open(&config);
    assert_eq!(
        view(&authority, &binding).phase,
        p::BootstrapClaimPhaseV2::Observed
    );
    inventory(&authority, &binding);
    assert_eq!(
        view(&authority, &binding).phase,
        p::BootstrapClaimPhaseV2::Observed
    );
    append(&authority, selection(&binding)).unwrap();
    assert_eq!(
        view(&authority, &binding).phase,
        p::BootstrapClaimPhaseV2::Selected
    );
    append(
        &authority,
        Fact::ClaimDataPrepared {
            prepared: prepared(&binding),
            at: NOW,
        },
    )
    .unwrap();
    let data_view = view(&authority, &binding);
    assert_eq!(data_view.phase, p::BootstrapClaimPhaseV2::DataPrepared);
    assert!(data_view.receipt.is_none() && data_view.active_data_binding_digest.is_none());
    append(
        &authority,
        Fact::ClaimCommittedV2 {
            commit: committed(&binding),
            at: NOW,
        },
    )
    .unwrap();
    let commit_view = view(&authority, &binding);
    assert_eq!(commit_view.phase, p::BootstrapClaimPhaseV2::Committed);
    assert!(commit_view.receipt.is_some());
    assert!(commit_view.activation.is_none() && commit_view.active_data_binding_digest.is_none());
    append(
        &authority,
        Fact::ClaimActivatedV2 {
            activation: activation(&binding),
            at: NOW,
        },
    )
    .unwrap();
    let active = view(&authority, &binding);
    assert_eq!(active.phase, p::BootstrapClaimPhaseV2::Activated);
    assert_eq!(
        active.active_data_binding_digest,
        Some(digest("target-binding"))
    );
    drop(authority);
    assert_eq!(
        Journal::open_v1(&config.journal_path, &config.install_ref, key)
            .err()
            .unwrap()
            .0,
        Code::RecoveryRequired
    );
    let stored = std::fs::read(&config.journal_path).unwrap();
    assert!(stored.starts_with(&prefix));
    let authority = open(&config);
    assert_eq!(view(&authority, &binding), active);
    assert_eq!(bytes(&authority), stored);
    assert_eq!(
        std::fs::read_dir(&config.new_brain_directory)
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn restart_at_each_stage_is_read_only_and_unactivated_data_requires_recovery() {
    for stop_at in 0..=5 {
        let (_directory, config, authority, binding) = setup();
        if stop_at >= 1 {
            inventory(&authority, &binding);
        }
        if stop_at >= 2 {
            append(&authority, selection(&binding)).unwrap();
        }
        if stop_at >= 3 {
            append(
                &authority,
                Fact::ClaimDataPrepared {
                    prepared: prepared(&binding),
                    at: NOW,
                },
            )
            .unwrap();
        }
        if stop_at >= 4 {
            append(
                &authority,
                Fact::ClaimCommittedV2 {
                    commit: committed(&binding),
                    at: NOW,
                },
            )
            .unwrap();
        }
        if stop_at >= 5 {
            append(
                &authority,
                Fact::ClaimActivatedV2 {
                    activation: activation(&binding),
                    at: NOW,
                },
            )
            .unwrap();
        }
        let stored = bytes(&authority);
        drop(authority);
        let authority = open(&config);
        let result = view(&authority, &binding);
        let expected = match stop_at {
            0 | 1 => p::BootstrapClaimPhaseV2::Observed,
            2 => p::BootstrapClaimPhaseV2::Selected,
            3 | 4 => p::BootstrapClaimPhaseV2::RecoveryRequired,
            _ => p::BootstrapClaimPhaseV2::Activated,
        };
        assert_eq!(result.phase, expected);
        if stop_at == 3 || stop_at == 4 {
            assert!(result.issue_ref.is_some());
            assert!(result.activation.is_none() && result.active_data_binding_digest.is_none());
            assert_rejected_without_write(
                &authority,
                &config,
                Fact::ClaimActivatedV2 {
                    activation: activation(&binding),
                    at: NOW,
                },
            );
            assert_rejected_without_write(
                &authority,
                &config,
                Fact::ClaimCommittedV2 {
                    commit: committed(&binding),
                    at: NOW,
                },
            );
        }
        assert_eq!(bytes(&authority), stored);
    }
}

#[test]
fn verified_backup_uses_the_same_bound_commit_and_activation_lane() {
    let (_directory, config, authority, binding) = setup();
    let observed = catalog(p::BootstrapCandidateKind::VerifiedBackup);
    let candidate = observed.candidates[0].clone();
    append(
        &authority,
        Fact::CandidateInventoryObserved {
            operation: binding.operation_id.clone(),
            catalog: observed,
            at: NOW,
        },
    )
    .unwrap();
    let mut selected = selection(&binding);
    if let Fact::CandidateSelectedV2 {
        selection,
        candidate: item,
        ..
    } = &mut selected
    {
        selection.expected_candidate_digest = candidate.candidate_digest.clone();
        *item = candidate.clone();
    }
    append(&authority, selected).unwrap();
    let mut data = prepared(&binding);
    data.candidate_digest = candidate.candidate_digest.clone();
    append(
        &authority,
        Fact::ClaimDataPrepared {
            prepared: data,
            at: NOW,
        },
    )
    .unwrap();
    let mut commit = committed(&binding);
    commit.receipt.candidate_kind = p::BootstrapCandidateKind::VerifiedBackup;
    commit.receipt.candidate_digest = candidate.candidate_digest;
    let mut active = activation(&binding);
    active.receipt_digest = commit.receipt.digest().unwrap();
    append(&authority, Fact::ClaimCommittedV2 { commit, at: NOW }).unwrap();
    append(
        &authority,
        Fact::ClaimActivatedV2 {
            activation: active,
            at: NOW,
        },
    )
    .unwrap();
    let result = view(&authority, &binding);
    assert_eq!(result.phase, p::BootstrapClaimPhaseV2::Activated);
    assert_eq!(
        result.receipt.as_ref().unwrap().candidate_kind,
        p::BootstrapCandidateKind::VerifiedBackup
    );
    drop(authority);
    assert_eq!(view(&open(&config), &binding), result);
}

#[test]
fn selection_rejects_wrong_session_digest_epoch_owner_or_unverified_candidate() {
    for case in 0..6 {
        let (_directory, config, authority, binding) = setup();
        inventory(&authority, &binding);
        let mut fact = selection(&binding);
        let Fact::CandidateSelectedV2 {
            selection,
            candidate,
            ..
        } = &mut fact
        else {
            unreachable!()
        };
        match case {
            0 => selection.session.setup_digest = digest("other-session"),
            1 => selection.session.operation_id = "claim:other-operation".into(),
            2 => selection.expected_candidate_digest = digest("stale-candidate"),
            3 => candidate.verification_epoch += 1,
            4 => candidate.owner_ref = Some(p::OwnerPrincipalRef("owner:substituted".into())),
            _ => {
                candidate.status = p::BootstrapCandidateStatus::Unverifiable;
                candidate.issue_ref = Some("issue:busy-source".into());
            }
        }
        assert_rejected_without_write(&authority, &config, fact);
        assert_eq!(
            view(&authority, &binding).phase,
            p::BootstrapClaimPhaseV2::Observed
        );
    }
}

#[test]
fn inventory_rejects_stale_epoch_installation_drift_and_post_selection_refresh() {
    let (_directory, config, authority, binding) = setup();
    inventory(&authority, &binding);
    assert_rejected_without_write(
        &authority,
        &config,
        Fact::CandidateInventoryObserved {
            operation: binding.operation_id.clone(),
            catalog: catalog(p::BootstrapCandidateKind::ExistingBrain),
            at: NOW,
        },
    );
    let foreign = next_catalog("installation:foreign");
    assert_rejected_without_write(
        &authority,
        &config,
        Fact::CandidateInventoryObserved {
            operation: binding.operation_id.clone(),
            catalog: foreign,
            at: NOW,
        },
    );
    append(&authority, selection(&binding)).unwrap();
    assert_rejected_without_write(
        &authority,
        &config,
        Fact::CandidateInventoryObserved {
            operation: binding.operation_id.clone(),
            catalog: next_catalog("installation:claim-fixture"),
            at: NOW,
        },
    );
}

#[test]
fn prepared_and_commit_are_bound_to_candidate_owner_target_and_worker_result() {
    let (_directory, config, authority, binding) = setup();
    inventory(&authority, &binding);
    append(&authority, selection(&binding)).unwrap();
    for case in 0..4 {
        let mut data = prepared(&binding);
        match case {
            0 => data.candidate_digest = digest("changed-source"),
            1 => data.verification_epoch += 1,
            2 => data.migration_outcome = p::BootstrapMigrationOutcome::Unknown,
            _ => data.worker_receipt_digest = p::SchemaDigest("not-a-digest".into()),
        }
        assert_rejected_without_write(
            &authority,
            &config,
            Fact::ClaimDataPrepared {
                prepared: data,
                at: NOW,
            },
        );
    }
    append(
        &authority,
        Fact::ClaimDataPrepared {
            prepared: prepared(&binding),
            at: NOW,
        },
    )
    .unwrap();
    for case in 0..5 {
        let mut commit = committed(&binding);
        match case {
            0 => commit.receipt.target_binding_digest = digest("wrong-target"),
            1 => commit.receipt.candidate_kind = p::BootstrapCandidateKind::VerifiedBackup,
            2 => {
                commit.receipt.owner_ref = p::OwnerPrincipalRef("owner:substituted".into());
                commit.receipt.owner_binding_digest = p::canonical_digest(&(
                    "forme-bootstrap-owner-binding-v2",
                    &commit.receipt.owner_ref,
                ))
                .unwrap();
            }
            3 => commit.receipt.migration_outcome = p::BootstrapMigrationOutcome::Prepared,
            _ => commit.receipt.expires_at += 1,
        }
        assert_rejected_without_write(
            &authority,
            &config,
            Fact::ClaimCommittedV2 { commit, at: NOW },
        );
    }
    append(
        &authority,
        Fact::ClaimCommittedV2 {
            commit: committed(&binding),
            at: NOW,
        },
    )
    .unwrap();
    let mut wrong = activation(&binding);
    wrong.active_data_binding_digest = digest("different-active-data");
    assert_rejected_without_write(
        &authority,
        &config,
        Fact::ClaimActivatedV2 {
            activation: wrong,
            at: NOW,
        },
    );
}

#[test]
fn abort_before_data_preserves_history_but_cannot_clear_prepared_data() {
    let (directory, _config, authority, binding) = setup();
    inventory(&authority, &binding);
    append(&authority, selection(&binding)).unwrap();
    assert!(authority
        .issue_proof(&admin(&directory), "proof:blocked-rotation", NOW)
        .is_err());
    let legacy = session(&binding.operation_id);
    assert_eq!(
        choose(&authority, &legacy, NOW).unwrap_err().0,
        Code::Conflict
    );
    append(&authority, abort(&binding)).unwrap();
    assert_eq!(
        view(&authority, &binding).phase,
        p::BootstrapClaimPhaseV2::Aborted
    );
    let aborted_bytes = bytes(&authority);
    let proof = authority
        .issue_proof(&admin(&directory), "proof:after-abort", NOW)
        .unwrap();
    let next = session("claim:new-after-abort");
    reserve(&authority, &proof.proof, &next, NOW).unwrap();
    choose(&authority, &next, NOW).unwrap();
    commit(&authority, &next, "account:new-after-abort", NOW).unwrap();
    assert!(bytes(&authority).starts_with(&aborted_bytes));

    let (_directory, config, authority, binding) = setup();
    prepare(&authority, &binding);
    assert_rejected_without_write(&authority, &config, abort(&binding));
    assert_eq!(
        view(&authority, &binding).phase,
        p::BootstrapClaimPhaseV2::DataPrepared
    );
}

#[test]
fn query_requires_service_and_exact_setup_binding_without_appending() {
    let (_directory, _config, authority, binding) = setup();
    inventory(&authority, &binding);
    append(&authority, selection(&binding)).unwrap();
    let stored = bytes(&authority);
    let query = || p::BootstrapClaimQueryV2 {
        schema_version: p::SchemaVersion(2),
        session: binding.clone(),
    };
    let forged = BootstrapServiceAuth(digest("other-service"));
    assert_eq!(
        authority
            .claim_state_v2(&forged, query(), NOW)
            .unwrap_err()
            .0,
        Code::Unauthenticated
    );
    let service = authority.authenticate_service(WEB).unwrap();
    let mut wrong = query();
    wrong.session.setup_digest = digest("wrong-setup");
    assert_eq!(
        authority
            .claim_state_v2(&service, wrong, NOW)
            .unwrap_err()
            .0,
        Code::Conflict
    );
    view(&authority, &binding);
    assert_eq!(bytes(&authority), stored);
}

#[test]
fn partial_unknown_and_downgraded_journals_are_preserved_and_fail_closed() {
    for case in 0..4 {
        let (_directory, config, authority, binding) = setup();
        inventory(&authority, &binding);
        let key = authority
            .lock(NOW)
            .unwrap()
            .state
            .binding
            .as_ref()
            .unwrap()
            .1
            .clone();
        drop(authority);
        let mut bytes = std::fs::read(&config.journal_path).unwrap();
        if case == 0 {
            bytes.pop();
        } else {
            let mut records: Vec<serde_json::Value> = bytes
                .split(|b| *b == b'\n')
                .filter(|b| !b.is_empty())
                .map(|b| serde_json::from_slice(b).unwrap())
                .collect();
            let record = records.last_mut().unwrap();
            record["schema_version"] = match case {
                1 => 3.into(),
                2 => 1.into(),
                _ => 2.into(),
            };
            if case == 3 {
                record["fact"]["kind"] = "FutureClaimFact".into();
            }
            bytes.clear();
            for record in records {
                bytes.extend(serde_json::to_vec(&record).unwrap());
                bytes.push(b'\n');
            }
        }
        std::fs::write(&config.journal_path, &bytes).unwrap();
        assert_eq!(
            Journal::open(&config.journal_path, &config.install_ref, key)
                .err()
                .unwrap()
                .0,
            Code::RecoveryRequired
        );
        assert_eq!(std::fs::read(&config.journal_path).unwrap(), bytes);
    }
}

#[test]
fn hash_valid_v1_fact_cannot_downgrade_a_v2_journal() {
    let (_directory, config, authority, binding) = setup();
    inventory(&authority, &binding);
    let key = authority
        .lock(NOW)
        .unwrap()
        .state
        .binding
        .as_ref()
        .unwrap()
        .1
        .clone();
    let mut stored = bytes(&authority);
    let head: serde_json::Value = serde_json::from_slice(
        stored
            .split(|b| *b == b'\n')
            .rfind(|b| !b.is_empty())
            .unwrap(),
    )
    .unwrap();
    let sequence = head["sequence"].as_u64().unwrap() + 1;
    let previous: p::SchemaDigest = serde_json::from_value(head["digest"].clone()).unwrap();
    let fact = Fact::ProofIssued {
        operation: "proof:valid-v1-downgrade".into(),
        digest: digest("rotated-proof"),
        at: NOW,
        expires: NOW + PROOF_TTL,
    };
    let record_digest = p::canonical_digest(&(
        "forme-install-claim-journal-v1",
        p::SchemaVersion(1),
        sequence,
        &previous,
        &fact,
    ))
    .unwrap();
    stored.extend(
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1, "sequence": sequence, "previous": previous,
            "fact": fact, "digest": record_digest,
        }))
        .unwrap(),
    );
    stored.push(b'\n');
    drop(authority);
    std::fs::write(&config.journal_path, &stored).unwrap();
    assert_eq!(
        Journal::open(&config.journal_path, &config.install_ref, key)
            .err()
            .unwrap()
            .0,
        Code::RecoveryRequired,
    );
    assert_eq!(std::fs::read(&config.journal_path).unwrap(), stored);
}
