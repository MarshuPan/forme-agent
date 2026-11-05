//! New intent/confirmation transitions and each durable interruption boundary.
use super::*;

fn intent(binding: &p::BootstrapSessionBindingV2) -> p::BootstrapDataIntentV2 {
    let candidate = catalog(p::BootstrapCandidateKind::ExistingBrain).candidates[0].clone();
    p::BootstrapDataIntentV2 {
        schema_version: p::SchemaVersion(2),
        install_ref: "installation:claim-fixture".into(),
        request: p::BootstrapClaimRequestV2 {
            schema_version: p::SchemaVersion(2),
            session: binding.clone(),
            candidate_ref: candidate.candidate_ref.clone(),
            expected_candidate_digest: candidate.candidate_digest.clone(),
            account_commitment: committed(binding).receipt.account_commitment,
        },
        candidate,
    }
}
fn confirmation(binding: &p::BootstrapSessionBindingV2) -> p::BootstrapDataBindingConfirmationV2 {
    let receipt = committed(binding).receipt;
    p::BootstrapDataBindingConfirmationV2 {
        schema_version: p::SchemaVersion(2),
        install_ref: receipt.install_ref.clone(),
        operation_id: receipt.operation_id.clone(),
        receipt_digest: receipt.digest().unwrap(),
        expected_config_revision: 0,
        config_binding_digest: digest("config-snapshot"),
        target_binding_digest: receipt.target_binding_digest,
        execution_fence_digest: digest("execution-fence"),
        verification_digest: digest("actual-target-verification"),
    }
}
fn start(authority: &BootstrapAuthority, binding: &p::BootstrapSessionBindingV2) {
    inventory(authority, binding);
    append(authority, selection(binding)).unwrap();
    append(
        authority,
        Fact::ClaimDataIntentV2 {
            intent: intent(binding),
            at: NOW,
        },
    )
    .unwrap();
}

#[test]
fn offline_activation_restart_at_every_fact_never_redispatches_or_invents_confirmation() {
    for stop_at in 1..=5 {
        let (_directory, config, authority, binding) = setup();
        start(&authority, &binding);
        if stop_at >= 2 {
            append(
                &authority,
                Fact::ClaimDataPrepared {
                    prepared: prepared(&binding),
                    at: NOW,
                },
            )
            .unwrap();
        }
        if stop_at >= 3 {
            append(
                &authority,
                Fact::ClaimCommittedV2 {
                    commit: committed(&binding),
                    at: NOW,
                },
            )
            .unwrap();
        }
        if stop_at >= 4 {
            append(
                &authority,
                Fact::ClaimDataBindingConfirmedV2 {
                    confirmation: confirmation(&binding),
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
        let before = bytes(&authority);
        drop(authority);
        let authority = open(&config);
        let service = authority.authenticate_service(WEB).unwrap();
        let query = p::BootstrapClaimQueryV2 {
            schema_version: p::SchemaVersion(2),
            session: binding.clone(),
        };
        let result = authority
            .claim_state_v2(&service, query.clone(), NOW + 2 * SETUP_TTL)
            .unwrap();
        if stop_at == 5 {
            assert_eq!(result.phase, p::BootstrapClaimPhaseV2::Activated);
            let signed = authority
                .claim_activation_v2(&service, query.clone(), NOW + 2 * SETUP_TTL)
                .unwrap();
            assert_eq!(signed.receipt, committed(&binding).receipt);
            assert_eq!(
                authority
                    .claim_activation_v2(&service, query, NOW + 3 * SETUP_TTL)
                    .unwrap(),
                signed
            );
        } else {
            assert_eq!(result.phase, p::BootstrapClaimPhaseV2::RecoveryRequired);
            assert!(result.activation.is_none());
            assert_eq!(
                authority
                    .claim_activation_v2(&service, query, NOW + 2 * SETUP_TTL)
                    .unwrap_err()
                    .0,
                Code::RecoveryRequired
            );
        }
        assert_eq!(bytes(&authority), before);
        assert_eq!(
            std::fs::read_dir(&config.new_brain_directory)
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn activation_requires_original_account_and_exact_offline_confirmation_before_signing() {
    let (_directory, config, authority, binding) = setup();
    start(&authority, &binding);
    assert_rejected_without_write(&authority, &config, abort(&binding));
    append(
        &authority,
        Fact::ClaimDataPrepared {
            prepared: prepared(&binding),
            at: NOW,
        },
    )
    .unwrap();
    let mut replaced = committed(&binding);
    replaced.receipt.account_commitment = digest("different-account");
    assert_rejected_without_write(
        &authority,
        &config,
        Fact::ClaimCommittedV2 {
            commit: replaced,
            at: NOW,
        },
    );
    append(
        &authority,
        Fact::ClaimCommittedV2 {
            commit: committed(&binding),
            at: NOW,
        },
    )
    .unwrap();
    assert_rejected_without_write(
        &authority,
        &config,
        Fact::ClaimActivatedV2 {
            activation: activation(&binding),
            at: NOW,
        },
    );
    for case in 0..4 {
        let mut wrong = confirmation(&binding);
        match case {
            0 => wrong.receipt_digest = digest("different-receipt"),
            1 => wrong.target_binding_digest = digest("different-target"),
            2 => wrong.install_ref = "installation:foreign".into(),
            _ => wrong.expected_config_revision = 1,
        }
        assert_rejected_without_write(
            &authority,
            &config,
            Fact::ClaimDataBindingConfirmedV2 {
                confirmation: wrong,
                at: NOW,
            },
        );
    }
    append(
        &authority,
        Fact::ClaimDataBindingConfirmedV2 {
            confirmation: confirmation(&binding),
            at: NOW,
        },
    )
    .unwrap();
    append(
        &authority,
        Fact::ClaimActivatedV2 {
            activation: activation(&binding),
            at: NOW,
        },
    )
    .unwrap();
    assert_eq!(
        view(&authority, &binding).phase,
        p::BootstrapClaimPhaseV2::Activated
    );
}
