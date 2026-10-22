//! Repository-owned authentication fixtures, never live owner data.
use super::*;
use ed25519_dalek::{Signer, SigningKey};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

const PASSWORD: &str = "fixture owner password 92";
const NOW: i64 = 1_000_000;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap();
        let path = root.join(format!(
            "forme-web-auth-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let root = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(self.0.parent(), Some(root.as_path()));
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("forme-web-auth-"));
        assert!(!std::fs::symlink_metadata(&self.0)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(self.0.canonicalize().unwrap(), self.0);
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[117; 32])
}
fn open(directory: &Directory) -> WebIdentityStore {
    WebIdentityStore::open(
        &directory.0,
        "installation:fixture",
        signing_key().verifying_key(),
    )
    .unwrap()
}
fn signed(commitment: p::SchemaDigest) -> p::SignedWebClaimCommit {
    let receipt = p::WebClaimCommit {
        schema_version: p::SchemaVersion(1),
        install_ref: "installation:fixture".into(),
        operation_id: "claim:first".into(),
        candidate_ref: "data:verified-fixture".into(),
        owner: p::OwnerPrincipalRef("owner:fixture".into()),
        account_commitment: commitment,
        expires_at: NOW + 60_000,
    };
    let bytes = p::canonical_digest(&("forme-web-claim-commit-v1", &receipt)).unwrap();
    p::SignedWebClaimCommit {
        schema_version: p::SchemaVersion(1),
        receipt,
        signature: signing_key().sign(bytes.0.as_bytes()).to_bytes().to_vec(),
    }
}
fn claimed(directory: &Directory) -> WebIdentityStore {
    let store = open(directory);
    let commitment = store
        .prepare_claim(
            "claim:first",
            "fixture",
            SensitivePassword::new(PASSWORD.into()),
        )
        .unwrap();
    store.commit_claim_at(&signed(commitment), NOW).unwrap();
    store
}
fn login(store: &WebIdentityStore, now: i64) -> SessionCookie {
    store
        .login_at(
            SensitiveLoginRequest {
                login_name: "fixture".into(),
                password: SensitivePassword::new(PASSWORD.into()),
            },
            now,
        )
        .unwrap()
}
fn owner(store: &WebIdentityStore, login: &SessionCookie, now: i64) -> AuthenticatedWebOwner {
    store
        .authenticate_at(
            &login.cookie,
            WebRequestBinding {
                mutation: true,
                csrf: Some(SensitiveCookie::new(
                    login.csrf.expose_for_transport().into(),
                )),
            },
            now,
        )
        .unwrap()
}

#[test]
fn product_operation_confirmation_consumes_only_exact_password_bound_proof() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let clock = super::now();
    let session = login(&store, clock);
    let owner = owner(&store, &session, clock);
    let operation = p::canonical_digest(&("core-start", 1)).unwrap();
    assert!(matches!(
        store.authorize_product_operation(
            &owner,
            SensitivePassword::new("wrong fixture password".into()),
            &operation
        ),
        Err(AuthError::Unauthenticated)
    ));
    let count = || {
        store
            .database()
            .unwrap()
            .connection
            .query_row("SELECT count(*) FROM reauth", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    assert_eq!(count(), 0);
    store
        .authorize_product_operation(&owner, SensitivePassword::new(PASSWORD.into()), &operation)
        .unwrap();
    assert_eq!(count(), 0);
    let proof = store
        .reauthenticate(&owner, SensitivePassword::new(PASSWORD.into()), &operation)
        .unwrap();
    let other = p::canonical_digest(&("core-start", 2)).unwrap();
    let db = store.database().unwrap();
    assert!(storage::claim_proof(&db.connection, &owner, &proof, &other, super::now()).is_err());
    storage::claim_proof(&db.connection, &owner, &proof, &operation, super::now()).unwrap();
    assert!(
        storage::claim_proof(&db.connection, &owner, &proof, &operation, super::now()).is_err()
    );
}

#[test]
fn revoked_session_cannot_confirm_product_operation_even_with_correct_password() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let clock = super::now();
    let session = login(&store, clock);
    let owner = owner(&store, &session, clock);
    store.logout(&owner).unwrap();
    let operation = p::canonical_digest(&"core-start-after-logout").unwrap();
    assert!(matches!(
        store.authorize_product_operation(
            &owner,
            SensitivePassword::new(PASSWORD.into()),
            &operation
        ),
        Err(AuthError::Unauthenticated)
    ));
}

#[test]
fn identity_claim_requires_pinned_signature_and_exact_account_commitment() {
    let directory = Directory::new();
    let store = open(&directory);
    let commitment = store
        .prepare_claim(
            "claim:first",
            "fixture",
            SensitivePassword::new(PASSWORD.into()),
        )
        .unwrap();
    let grant = signed(commitment.clone());
    let mut foreign = grant.clone();
    foreign.receipt.owner = p::OwnerPrincipalRef("owner:foreign".into());
    assert!(matches!(
        store.commit_claim_at(&foreign, NOW),
        Err(AuthError::Unauthenticated)
    ));
    assert!(account(&store.database().unwrap().connection)
        .unwrap()
        .is_none());
    let mut foreign = grant.clone();
    foreign.receipt.account_commitment = p::canonical_digest(&"wrong account").unwrap();
    let bytes = p::canonical_digest(&("forme-web-claim-commit-v1", &foreign.receipt)).unwrap();
    foreign.signature = signing_key().sign(bytes.0.as_bytes()).to_bytes().to_vec();
    assert!(matches!(
        store.commit_claim_at(&foreign, NOW),
        Err(AuthError::Conflict)
    ));
    assert!(matches!(
        store.commit_claim_at(&grant, NOW + 60_000),
        Err(AuthError::Expired)
    ));
    assert_eq!(store.commit_claim_at(&grant, NOW).unwrap().auth_epoch, 1);
    assert_eq!(store.commit_claim_at(&grant, NOW).unwrap().auth_epoch, 1);
    assert!(matches!(
        store.prepare_claim(
            "claim:second",
            "another",
            SensitivePassword::new(PASSWORD.into())
        ),
        Err(AuthError::Conflict)
    ));
}

#[test]
fn identity_pending_claim_survives_restart_and_cannot_change_semantics() {
    let directory = Directory::new();
    let store = open(&directory);
    let commitment = store
        .prepare_claim(
            "claim:first",
            "fixture",
            SensitivePassword::new(PASSWORD.into()),
        )
        .unwrap();
    drop(store);
    let store = open(&directory);
    assert_eq!(
        store
            .prepare_claim(
                "claim:first",
                "fixture",
                SensitivePassword::new(PASSWORD.into())
            )
            .unwrap(),
        commitment
    );
    assert!(matches!(
        store.prepare_claim(
            "claim:first",
            "other",
            SensitivePassword::new(PASSWORD.into())
        ),
        Err(AuthError::Conflict)
    ));
    store.commit_claim_at(&signed(commitment), NOW).unwrap();
    assert_eq!(login(&store, NOW).view.owner.0, "owner:fixture");
}

#[test]
fn identity_logout_revocation_survives_restart_and_secrets_are_not_stored() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let login = login(&store, NOW);
    let cookie_copy = login.cookie.expose_for_transport().to_owned();
    let csrf_copy = login.csrf.expose_for_transport().to_owned();
    let authenticated = owner(&store, &login, NOW + 1);
    store.logout_at(&authenticated, NOW + 2).unwrap();
    let pending = store.pending_fences().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].target_session.as_ref(),
        Some(&pending[0].source_session)
    );
    let operation = pending[0].operation.clone();
    drop(store);
    let store = open(&directory);
    assert_eq!(store.pending_fences().unwrap().len(), 1);
    store.acknowledge_fence(&operation).unwrap();
    assert!(store.pending_fences().unwrap().is_empty());
    assert!(matches!(
        store.authenticate_at(
            &login.cookie,
            WebRequestBinding {
                mutation: false,
                csrf: None
            },
            NOW + 3
        ),
        Err(AuthError::Unauthenticated)
    ));
    drop(store);
    for entry in std::fs::read_dir(&directory.0).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        for secret in [PASSWORD, &cookie_copy, &csrf_copy] {
            assert!(!bytes
                .windows(secret.len())
                .any(|window| window == secret.as_bytes()));
        }
    }
}

#[test]
fn identity_tampered_fence_outbox_requires_local_recovery() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let login = login(&store, NOW);
    let authenticated = owner(&store, &login, NOW + 1);
    store.logout_at(&authenticated, NOW + 2).unwrap();
    store
        .database()
        .unwrap()
        .connection
        .execute("UPDATE fence_outbox SET minimum_epoch=1", [])
        .unwrap();
    assert!(matches!(
        store.pending_fences(),
        Err(AuthError::RecoveryRequired)
    ));
}

#[test]
fn identity_session_enforces_csrf_idle_absolute_and_clock_rollback() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let first = login(&store, NOW);
    assert!(matches!(
        store.authenticate_at(
            &first.cookie,
            WebRequestBinding {
                mutation: true,
                csrf: None
            },
            NOW
        ),
        Err(AuthError::CsrfRejected)
    ));
    assert!(matches!(
        store.authenticate_at(
            &first.cookie,
            WebRequestBinding {
                mutation: false,
                csrf: None
            },
            NOW - 1
        ),
        Err(AuthError::Unauthenticated)
    ));
    assert!(matches!(
        store.authenticate_at(
            &first.cookie,
            WebRequestBinding {
                mutation: false,
                csrf: None
            },
            NOW + IDLE_TTL
        ),
        Err(AuthError::Unauthenticated)
    ));
    let second = login(&store, NOW);
    for step in 1..=24 {
        owner(&store, &second, NOW + step * (IDLE_TTL - 60_000));
    }
    assert!(matches!(
        store.authenticate_at(
            &second.cookie,
            WebRequestBinding {
                mutation: false,
                csrf: None
            },
            NOW + ABSOLUTE_TTL
        ),
        Err(AuthError::Unauthenticated)
    ));
}

#[test]
fn identity_reauth_is_operation_bound_single_use_and_epoch_fenced() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let first = login(&store, NOW);
    let second = login(&store, NOW);
    let authenticated = owner(&store, &first, NOW + 1);
    let operation = WebIdentityStore::revoke_operation(&authenticated).unwrap();
    let proof = store
        .reauthenticate_at(
            &authenticated,
            SensitivePassword::new(PASSWORD.into()),
            &operation,
            NOW + 2,
        )
        .unwrap();
    let copied = proof.0.expose_for_transport().to_owned();
    let wrong = p::canonical_digest(&"other operation").unwrap();
    assert!(matches!(
        store.revoke_at(
            &authenticated,
            RevokeSessions {
                operation_digest: wrong,
                proof
            },
            NOW + 3
        ),
        Err(AuthError::Conflict)
    ));
    let restored = ReauthenticationProof(SensitiveCookie::new(copied.clone()));
    assert_eq!(
        store
            .revoke_at(
                &authenticated,
                RevokeSessions {
                    operation_digest: operation.clone(),
                    proof: restored
                },
                NOW + 4
            )
            .unwrap()
            .auth_epoch,
        2
    );
    assert!(store
        .authenticate_at(
            &second.cookie,
            WebRequestBinding {
                mutation: false,
                csrf: None
            },
            NOW + 5
        )
        .is_err());
    assert!(store
        .revoke_at(
            &authenticated,
            RevokeSessions {
                operation_digest: operation,
                proof: ReauthenticationProof(SensitiveCookie::new(copied))
            },
            NOW + 5
        )
        .is_err());
    assert_eq!(login(&store, NOW + 6).view.auth_epoch, 2);
}

#[test]
fn identity_password_change_binds_new_hash_and_preserves_owner_across_restart() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let first = login(&store, NOW);
    let authenticated = owner(&store, &first, NOW + 1);
    let replacement = "replacement fixture password 39";
    let prepared = store
        .prepare_password_change_at(
            &authenticated,
            SensitivePassword::new(replacement.into()),
            NOW + 2,
        )
        .unwrap();
    let operation = prepared.operation_digest().clone();
    let proof = store
        .reauthenticate_at(
            &authenticated,
            SensitivePassword::new(PASSWORD.into()),
            &operation,
            NOW + 3,
        )
        .unwrap();
    let revision = store
        .change_password_at(
            &authenticated,
            RevokeSessions {
                operation_digest: operation,
                proof,
            },
            prepared,
            NOW + 4,
        )
        .unwrap();
    assert_eq!(revision.credential_revision, 2);
    assert_eq!(revision.auth_epoch, 2);
    assert!(store
        .authenticate_at(
            &first.cookie,
            WebRequestBinding {
                mutation: false,
                csrf: None
            },
            NOW + 5
        )
        .is_err());
    drop(store);
    let store = open(&directory);
    assert!(matches!(
        store.login_at(
            SensitiveLoginRequest {
                login_name: "fixture".into(),
                password: SensitivePassword::new(PASSWORD.into())
            },
            NOW + 6
        ),
        Err(AuthError::Unauthenticated)
    ));
    let new = store
        .login_at(
            SensitiveLoginRequest {
                login_name: "fixture".into(),
                password: SensitivePassword::new(replacement.into()),
            },
            NOW + 7,
        )
        .unwrap();
    assert_eq!(new.view.owner.0, "owner:fixture");
    assert_eq!(new.view.auth_epoch, 2);
}

#[test]
fn identity_foreign_database_and_second_writer_are_rejected_without_repair() {
    let directory = Directory::new();
    let store = open(&directory);
    assert!(matches!(
        WebIdentityStore::open(
            &directory.0,
            "installation:fixture",
            signing_key().verifying_key()
        ),
        Err(AuthError::Busy)
    ));
    drop(store);
    assert!(matches!(
        WebIdentityStore::open(
            &directory.0,
            "installation:other",
            signing_key().verifying_key()
        ),
        Err(AuthError::RecoveryRequired)
    ));
    assert!(matches!(
        WebIdentityStore::open(
            &directory.0,
            "installation:fixture",
            SigningKey::from_bytes(&[118; 32]).verifying_key()
        ),
        Err(AuthError::RecoveryRequired)
    ));
    let foreign = Directory::new();
    let path = foreign.0.join("identity.sqlite");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute("CREATE TABLE foreign_data (value TEXT)", [])
        .unwrap();
    connection
        .execute("INSERT INTO foreign_data VALUES ('preserve')", [])
        .unwrap();
    drop(connection);
    let before = std::fs::read(&path).unwrap();
    assert!(matches!(
        WebIdentityStore::open(
            &foreign.0,
            "installation:fixture",
            signing_key().verifying_key()
        ),
        Err(AuthError::RecoveryRequired)
    ));
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn identity_password_capacity_is_held_until_the_actual_job_finishes() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let one = store.password_slot().unwrap();
    let two = store.password_slot().unwrap();
    assert!(matches!(
        store.login_at(
            SensitiveLoginRequest {
                login_name: "fixture".into(),
                password: SensitivePassword::new(PASSWORD.into())
            },
            NOW
        ),
        Err(AuthError::Busy)
    ));
    drop(one);
    assert!(store.password_slot().is_ok());
    drop(two);
}

#[test]
fn identity_password_bounds_do_not_trim_and_corrupt_kdf_parameters_fail_closed() {
    assert!(password::validate_password("short").is_err());
    assert!(password::validate_password(&"界".repeat(129)).is_err());
    let record = PasswordRecord::new(" trailing fixture password ").unwrap();
    assert!(record.verify(" trailing fixture password ").unwrap());
    assert!(!record.verify("trailing fixture password").unwrap());
    let mut data = serde_json::to_value(record).unwrap();
    data["memory_kib"] = serde_json::json!(4_000_000);
    let corrupted: PasswordRecord = serde_json::from_value(data).unwrap();
    assert!(matches!(
        corrupted.verify(PASSWORD),
        Err(AuthError::RecoveryRequired)
    ));
}

#[test]
fn identity_expired_or_other_session_reauth_cannot_revoke() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let first = login(&store, NOW);
    let second = login(&store, NOW);
    let one = owner(&store, &first, NOW + 1);
    let two = owner(&store, &second, NOW + 1);
    let operation = WebIdentityStore::revoke_operation(&one).unwrap();
    let proof = store
        .reauthenticate_at(
            &one,
            SensitivePassword::new(PASSWORD.into()),
            &operation,
            NOW + 2,
        )
        .unwrap();
    let copy = proof.0.expose_for_transport().to_owned();
    assert!(matches!(
        store.revoke_at(
            &two,
            RevokeSessions {
                operation_digest: operation.clone(),
                proof
            },
            NOW + 3
        ),
        Err(AuthError::Unauthenticated)
    ));
    assert!(matches!(
        store.revoke_at(
            &one,
            RevokeSessions {
                operation_digest: operation,
                proof: ReauthenticationProof(SensitiveCookie::new(copy))
            },
            NOW + 2 + REAUTH_TTL
        ),
        Err(AuthError::Unauthenticated)
    ));
    assert_eq!(login(&store, NOW + 4 + REAUTH_TTL).view.auth_epoch, 1);
}

#[test]
fn identity_change_password_rejects_a_proof_for_another_prepared_hash() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let login = login(&store, NOW);
    let authenticated = owner(&store, &login, NOW + 1);
    let first = store
        .prepare_password_change_at(
            &authenticated,
            SensitivePassword::new("first replacement fixture 21".into()),
            NOW + 2,
        )
        .unwrap();
    let second = store
        .prepare_password_change_at(
            &authenticated,
            SensitivePassword::new("second replacement fixture 34".into()),
            NOW + 2,
        )
        .unwrap();
    let operation = first.operation_digest().clone();
    let proof = store
        .reauthenticate_at(
            &authenticated,
            SensitivePassword::new(PASSWORD.into()),
            &operation,
            NOW + 3,
        )
        .unwrap();
    let copy = proof.0.expose_for_transport().to_owned();
    assert!(matches!(
        store.change_password_at(
            &authenticated,
            RevokeSessions {
                operation_digest: operation.clone(),
                proof
            },
            second,
            NOW + 4
        ),
        Err(AuthError::Conflict)
    ));
    assert_eq!(
        store
            .change_password_at(
                &authenticated,
                RevokeSessions {
                    operation_digest: operation,
                    proof: ReauthenticationProof(SensitiveCookie::new(copy))
                },
                first,
                NOW + 5
            )
            .unwrap()
            .auth_epoch,
        2
    );
}

#[test]
fn identity_concurrent_authentication_keeps_one_live_session_without_false_clock_rollback() {
    let directory = Directory::new();
    let store = std::sync::Arc::new(claimed(&directory));
    let session = login(&store, now());
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            let cookie = SensitiveCookie::new(session.cookie.expose_for_transport().into());
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..16 {
                    let owner = store
                        .authenticate(
                            &cookie,
                            WebRequestBinding {
                                mutation: false,
                                csrf: None,
                            },
                        )
                        .unwrap();
                    assert_eq!(owner.view().owner.0, "owner:fixture");
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let count: i64 = store
        .database()
        .unwrap()
        .connection
        .query_row("SELECT count(*) FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let authenticated = store
        .authenticate(
            &session.cookie,
            WebRequestBinding {
                mutation: false,
                csrf: None,
            },
        )
        .unwrap();
    store.logout(&authenticated).unwrap();
    assert!(matches!(
        store.authenticate(
            &session.cookie,
            WebRequestBinding {
                mutation: false,
                csrf: None,
            }
        ),
        Err(AuthError::Unauthenticated)
    ));
}

#[test]
fn identity_login_challenge_is_csrf_bound_expiring_and_consumed_once_across_restart() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let challenge = store.login_challenge_at(NOW).unwrap();
    let valid_csrf = SensitiveCookie::new(challenge.view.csrf.clone());
    let wrong_csrf = SensitiveCookie::new("0".repeat(64));
    let request = || SensitiveLoginRequest {
        login_name: "fixture".into(),
        password: SensitivePassword::new(PASSWORD.into()),
    };
    let binding = |csrf, existing_session| LoginChallengeBinding {
        cookie: &challenge.cookie,
        csrf,
        attempt: &challenge.view.attempt_ref,
        existing_session,
    };
    assert!(matches!(
        store.login_with_time(request(), Some(NOW + 1), Some(binding(&wrong_csrf, None))),
        Err(AuthError::CsrfRejected)
    ));
    let session = store
        .login_with_time(request(), Some(NOW + 1), Some(binding(&valid_csrf, None)))
        .unwrap();
    drop(store);
    let store = open(&directory);
    assert!(matches!(
        store.login_with_time(request(), Some(NOW + 2), Some(binding(&valid_csrf, None))),
        Err(AuthError::Conflict)
    ));
    let recovered = store
        .login_with_time(
            request(),
            Some(NOW + 3),
            Some(binding(&valid_csrf, Some(&session.cookie))),
        )
        .unwrap();
    assert_eq!(
        recovered.cookie.expose_for_transport(),
        session.cookie.expose_for_transport()
    );
    assert!(matches!(
        store.login_with_time(
            request(),
            Some(NOW + 300_000),
            Some(binding(&valid_csrf, None))
        ),
        Err(AuthError::Expired)
    ));
    let count: i64 = store
        .database()
        .unwrap()
        .connection
        .query_row("SELECT count(*) FROM sessions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn identity_known_schema_upgrade_preserves_existing_account_and_session() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let session = login(&store, NOW);
    store
        .database()
        .unwrap()
        .connection
        .execute_batch(
            "DROP TABLE login_challenges; DROP TABLE setup_sessions; PRAGMA user_version=1;",
        )
        .unwrap();
    drop(store);
    let store = open(&directory);
    assert_eq!(
        owner(&store, &session, NOW + 1).view().owner.0,
        "owner:fixture"
    );
    let version: i64 = store
        .database()
        .unwrap()
        .connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 5);
    store.login_challenge_at(NOW + 2).unwrap();
}

#[test]
fn identity_setup_scope_is_distinct_and_candidate_binding_survives_restart() {
    let directory = Directory::new();
    let store = open(&directory);
    let setup = store.prepare_setup("claim:setup", None).unwrap();
    let other_cookie = SensitiveCookie::new("a".repeat(64));
    assert!(matches!(
        store.prepare_setup("claim:setup", Some(other_cookie)),
        Err(AuthError::Conflict)
    ));

    let now = super::now();
    store
        .activate_setup(
            &setup,
            &p::BootstrapReservationView {
                schema_version: p::SchemaVersion(1),
                operation_id: "claim:setup".into(),
                expires_at: now + 60_000,
            },
        )
        .unwrap();
    assert!(matches!(
        store.setup_access(
            &setup.cookie,
            WebRequestBinding {
                mutation: true,
                csrf: None,
            },
            false,
        ),
        Err(AuthError::CsrfRejected)
    ));
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
    let candidate = p::BootstrapDataCandidateView {
        schema_version: p::SchemaVersion(1),
        candidate_ref: "data:new-brain".into(),
        selected: true,
        available: true,
        issue_code: None,
    };
    store.record_setup_candidate(&access, &candidate).unwrap();
    store
        .prepare_claim(
            "claim:setup",
            "fixture",
            SensitivePassword::new(PASSWORD.into()),
        )
        .unwrap();
    assert!(store
        .setup_access(
            &setup.cookie,
            WebRequestBinding {
                mutation: false,
                csrf: None,
            },
            false,
        )
        .is_ok());
    let pending = store.pending_bootstrap().unwrap().unwrap();
    assert_eq!(pending.session.operation_id, "claim:setup");
    assert_eq!(pending.candidate_ref, "data:new-brain");
    drop(store);

    let store = open(&directory);
    let pending = store.pending_bootstrap().unwrap().unwrap();
    assert_eq!(pending.session.operation_id, "claim:setup");
    assert_eq!(pending.candidate_ref, "data:new-brain");
}

#[test]
fn identity_invalid_setup_proof_can_be_discarded_without_touching_claim_intent() {
    let directory = Directory::new();
    let store = open(&directory);
    let setup = store.prepare_setup("claim:invalid", None).unwrap();
    store.discard_unactivated_setup(&setup).unwrap();
    assert!(matches!(
        store.setup_access(
            &setup.cookie,
            WebRequestBinding {
                mutation: false,
                csrf: None,
            },
            false,
        ),
        Err(AuthError::Unauthenticated)
    ));
    assert!(store.pending_bootstrap().unwrap().is_none());
}
