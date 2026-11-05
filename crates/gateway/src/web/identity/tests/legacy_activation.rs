//! Original account/hash and pinned signature survive a lost activation reply.
use super::*;

fn candidate() -> p::BootstrapCandidateViewV2 {
    let owner = p::OwnerPrincipalRef("owner:historical-fixture".into());
    let mut result = p::BootstrapCandidateViewV2 {
        schema_version: p::SchemaVersion(2),
        candidate_ref: "candidate:old".into(),
        kind: p::BootstrapCandidateKind::ExistingBrain,
        status: p::BootstrapCandidateStatus::Available,
        candidate_digest: p::sha256_content_digest(b"pending"),
        verification_epoch: 7,
        owner_source: p::BootstrapOwnerSource::VerifiedFromCandidate,
        owner_binding_digest: Some(
            p::canonical_digest(&("forme-bootstrap-owner-binding-v2", &owner)).unwrap(),
        ),
        owner_ref: Some(owner),
        store_identity_digest: Some(p::sha256_content_digest(b"store")),
        schema_digest: Some(p::sha256_content_digest(b"schema")),
        protection_state: p::BootstrapProtectionState::Protected,
        source_observation_digest: p::sha256_content_digest(b"source"),
        issue_ref: None,
    };
    result.candidate_digest = result.expected_digest("installation:fixture").unwrap();
    result
}

fn prepare(store: &WebIdentityStore) -> p::SignedBootstrapClaimActivationV2 {
    let setup = store.prepare_setup("claim:legacy-account", None).unwrap();
    store
        .activate_setup(
            &setup,
            &p::BootstrapReservationView {
                schema_version: p::SchemaVersion(1),
                operation_id: setup.binding.operation_id.clone(),
                expires_at: super::super::now() + 60_000,
            },
        )
        .unwrap();
    let access = store
        .setup_access(
            &setup.cookie,
            WebRequestBinding {
                mutation: false,
                csrf: None,
            },
            false,
        )
        .unwrap();
    let candidate = candidate();
    store
        .record_setup_candidate_v2(&access, &candidate)
        .unwrap();
    let commitment = store
        .prepare_claim(
            &setup.binding.operation_id,
            "fixture",
            SensitivePassword::new(PASSWORD.into()),
        )
        .unwrap();
    let receipt = p::BootstrapClaimReceiptV2 {
        schema_version: p::SchemaVersion(2),
        install_ref: "installation:fixture".into(),
        operation_id: setup.binding.operation_id,
        candidate_ref: candidate.candidate_ref,
        candidate_kind: candidate.kind,
        candidate_digest: candidate.candidate_digest,
        owner_source: candidate.owner_source,
        owner_ref: candidate.owner_ref.unwrap(),
        owner_binding_digest: candidate.owner_binding_digest.unwrap(),
        account_commitment: commitment,
        target_binding_digest: p::sha256_content_digest(b"target"),
        migration_outcome: p::BootstrapMigrationOutcome::Recovered,
        // Immutable historical expiry intentionally precedes the current clock.
        expires_at: NOW,
    };
    let activation = p::BootstrapClaimActivationV2 {
        schema_version: p::SchemaVersion(2),
        operation_id: receipt.operation_id.clone(),
        receipt_digest: receipt.digest().unwrap(),
        active_data_binding_digest: receipt.target_binding_digest.clone(),
    };
    let mut grant = p::SignedBootstrapClaimActivationV2 {
        schema_version: p::SchemaVersion(2),
        receipt,
        activation,
        signature: vec![0; 64],
    };
    sign(&mut grant);
    grant
}

fn sign(grant: &mut p::SignedBootstrapClaimActivationV2) {
    let digest =
        p::SignedBootstrapClaimActivationV2::signing_digest(&grant.receipt, &grant.activation)
            .unwrap();
    grant.signature = signing_key().sign(digest.0.as_bytes()).to_bytes().to_vec();
}

#[test]
fn identity_legacy_activation_requires_pinned_signature_candidate_owner_and_original_account() {
    let directory = Directory::new();
    let store = open(&directory);
    let original = prepare(&store);
    for field in [
        "signature",
        "key",
        "account",
        "candidate",
        "owner",
        "target",
        "receipt",
        "install",
        "prepared",
    ] {
        let mut grant = original.clone();
        match field {
            "signature" => grant.signature[0] ^= 1,
            "key" => {
                let digest = p::SignedBootstrapClaimActivationV2::signing_digest(
                    &grant.receipt,
                    &grant.activation,
                )
                .unwrap();
                grant.signature = SigningKey::from_bytes(&[99; 32])
                    .sign(digest.0.as_bytes())
                    .to_bytes()
                    .to_vec();
            }
            "account" | "candidate" | "owner" | "install" | "prepared" => {
                match field {
                    "account" => {
                        grant.receipt.account_commitment =
                            p::sha256_content_digest(b"other-account")
                    }
                    "candidate" => {
                        grant.receipt.candidate_digest =
                            p::sha256_content_digest(b"other-candidate")
                    }
                    "install" => grant.receipt.install_ref = "installation:other".into(),
                    "prepared" => {
                        grant.receipt.migration_outcome = p::BootstrapMigrationOutcome::Prepared
                    }
                    _ => {
                        grant.receipt.owner_ref = p::OwnerPrincipalRef("owner:other".into());
                        grant.receipt.owner_binding_digest = p::canonical_digest(&(
                            "forme-bootstrap-owner-binding-v2",
                            &grant.receipt.owner_ref,
                        ))
                        .unwrap();
                    }
                }
                grant.activation.receipt_digest = grant.receipt.digest().unwrap();
                if field != "prepared" {
                    sign(&mut grant);
                }
            }
            "target" => {
                grant.activation.active_data_binding_digest =
                    p::sha256_content_digest(b"other-target")
            }
            "receipt" => {
                grant.activation.receipt_digest = p::sha256_content_digest(b"other-receipt")
            }
            _ => unreachable!(),
        }
        assert!(
            store.commit_claim_activation(&grant).is_err(),
            "accepted substitution: {field}"
        );
        assert!(account(&store.database().unwrap().connection)
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .pending_legacy_bootstrap()
                .unwrap()
                .unwrap()
                .account_commitment,
            original.receipt.account_commitment
        );
    }
    assert!(store
        .prepare_claim(
            &original.receipt.operation_id,
            "changed",
            SensitivePassword::new(PASSWORD.into())
        )
        .is_err());
    assert!(store.pending_bootstrap().unwrap().is_none());
    drop(store);
    let store = open(&directory);
    let revision = store.commit_claim_activation(&original).unwrap();
    assert_eq!(store.commit_claim_activation(&original).unwrap(), revision);
    assert!(store.pending_legacy_bootstrap().unwrap().is_none());
    let current = account(&store.database().unwrap().connection)
        .unwrap()
        .unwrap();
    assert_eq!(current.owner, original.receipt.owner_ref);
    assert!(current.password.verify(PASSWORD).unwrap());
    assert!(
        store
            .database()
            .unwrap()
            .connection
            .query_row("SELECT active FROM setup_sessions", [], |r| r
                .get::<_, i64>(0))
            .unwrap()
            == 0
    );
}

#[test]
fn identity_legacy_startup_queries_only_original_activation_without_redispatch() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let directory = Directory::new();
    let store = open(&directory);
    let grant = prepare(&store);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let credential_path = directory.0.join("fixture-setup.token");
    std::fs::write(&credential_path, "fixture-private-setup-0123456789abcdef").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&credential_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let expected = grant.clone();
    let child = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut block = [0; 2048];
        loop {
            let size = stream.read(&mut block).unwrap();
            assert!(size > 0);
            bytes.extend_from_slice(&block[..size]);
            if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let header = std::str::from_utf8(&bytes[..index]).unwrap();
                let len: usize = header
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                if bytes.len() >= index + 4 + len {
                    break;
                }
            }
            assert!(bytes.len() < 8192);
        }
        assert!(bytes.starts_with(b"POST /v1/setup/activation-v2 HTTP/1.1\r\n"));
        let body = serde_json::to_vec(&expected).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(&body).unwrap();
    });
    let client =
        crate::web::bootstrap::LocalBootstrapClient::connect(address, &credential_path).unwrap();
    store.recover_bootstrap(&client, None).unwrap();
    child.join().unwrap();
    assert_eq!(
        account(&store.database().unwrap().connection)
            .unwrap()
            .unwrap()
            .owner,
        grant.receipt.owner_ref
    );
}
