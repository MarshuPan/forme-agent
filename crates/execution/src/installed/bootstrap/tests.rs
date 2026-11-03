//! Self-authored isolated fixtures; no installed service or personal brain.
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::*;
use crate::installed::{HostCredentials, HostManagement, ProductHostBroker, SystemServiceManager};

const NOW: i64 = 1_000_000;
const ADMIN: &str = "fixture-local-administrator-credential-1";
const WEB: &str = "fixture-bootstrap-web-service-credential-1";
const AGENT: &str = "fixture-agent-execution-service-credential-1";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "forme-install-claim-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        assert_eq!(
            self.0.canonicalize().unwrap().parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("forme-install-claim-"));
        assert_eq!(
            self.0.canonicalize().unwrap(),
            std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join(self.0.file_name().unwrap())
        );
        assert!(!std::fs::symlink_metadata(&self.0)
            .unwrap()
            .file_type()
            .is_symlink());
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn fixture() -> (Directory, BootstrapAuthorityConfig) {
    let directory = Directory::new();
    let config = BootstrapAuthorityConfig {
        schema_version: p::SchemaVersion(1),
        install_ref: "installation:claim-fixture".into(),
        journal_path: directory.0.join("claim.journal"),
        signing_key_path: directory.0.join("issuer.key"),
        web_credential_path: directory.0.join("web.credential"),
        new_brain_directory: directory.0.join("new-brain"),
        candidate_ref: "data:new-fixture".into(),
        historical_brain_directory: None,
        verified_backup_directory: None,
    };
    write_private(&config.signing_key_path, &[117; 32]);
    write_private(&config.web_credential_path, WEB.as_bytes());
    std::fs::create_dir(&config.new_brain_directory).unwrap();
    (directory, config)
}

fn write_private(path: &std::path::Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn open(config: &BootstrapAuthorityConfig) -> BootstrapAuthority {
    BootstrapAuthority::open(
        config.clone(),
        p::canonical_digest(&ADMIN).unwrap(),
        Some(&p::canonical_digest(&AGENT).unwrap()),
    )
    .unwrap()
}

fn admin(directory: &Directory) -> HostAuth {
    let broker = ProductHostBroker::open(
        &directory.0.join("admin.journal"),
        "installation:claim-fixture",
        ADMIN.into(),
        p::ProductTarget::WindowsX86_64,
        "fixture-v1".into(),
    )
    .unwrap();
    broker.authenticate(ADMIN).unwrap()
}

fn session(operation: &str) -> p::BootstrapSessionBinding {
    p::BootstrapSessionBinding {
        schema_version: p::SchemaVersion(1),
        operation_id: operation.into(),
        setup_digest: p::canonical_digest(&("fixture-setup-session", operation)).unwrap(),
    }
}
fn reserve(
    authority: &BootstrapAuthority,
    proof: &str,
    binding: &p::BootstrapSessionBinding,
    now: i64,
) -> BootstrapResult<p::BootstrapReservationView> {
    authority.reserve(
        &authority.authenticate_service(WEB).unwrap(),
        p::BootstrapReserveRequest {
            schema_version: p::SchemaVersion(1),
            operation_id: binding.operation_id.clone(),
            proof: proof.into(),
            setup_digest: binding.setup_digest.clone(),
        },
        now,
    )
}
fn choose(
    authority: &BootstrapAuthority,
    binding: &p::BootstrapSessionBinding,
    now: i64,
) -> BootstrapResult<p::BootstrapDataCandidateView> {
    authority.select(
        &authority.authenticate_service(WEB).unwrap(),
        p::BootstrapDataChoice {
            schema_version: p::SchemaVersion(1),
            session: binding.clone(),
            candidate_ref: "data:new-fixture".into(),
        },
        now,
    )
}
fn commit(
    authority: &BootstrapAuthority,
    binding: &p::BootstrapSessionBinding,
    commitment: &str,
    now: i64,
) -> BootstrapResult<p::SignedWebClaimCommit> {
    authority.commit(
        &authority.authenticate_service(WEB).unwrap(),
        p::BootstrapCommitRequest {
            schema_version: p::SchemaVersion(1),
            session: binding.clone(),
            candidate_ref: "data:new-fixture".into(),
            account_commitment: p::canonical_digest(&commitment).unwrap(),
        },
        now,
    )
}

#[test]
fn claim_commits_before_signature_and_never_creates_a_brain() {
    let (directory, config) = fixture();
    let authority = open(&config);
    let proof = authority
        .issue_proof(&admin(&directory), "proof:one", NOW)
        .unwrap();
    let binding = session("claim:one");
    let reserved = reserve(&authority, &proof.proof, &binding, NOW).unwrap();
    assert_eq!(reserved.expires_at, NOW + SETUP_TTL);
    assert_eq!(
        reserve(&authority, &proof.proof, &binding, NOW + 1).unwrap(),
        reserved
    );
    let service = authority.authenticate_service(WEB).unwrap();
    assert!(
        !authority
            .candidate(&service, &binding, NOW)
            .unwrap()
            .selected
    );
    assert!(choose(&authority, &binding, NOW).unwrap().selected);
    let signed = commit(&authority, &binding, "account:one", NOW).unwrap();
    let message = p::canonical_digest(&("forme-web-claim-commit-v1", &signed.receipt)).unwrap();
    ed25519_dalek::VerifyingKey::from_bytes(&authority.verifying_key())
        .unwrap()
        .verify_strict(
            message.0.as_bytes(),
            &ed25519_dalek::Signature::from_slice(&signed.signature).unwrap(),
        )
        .unwrap();
    assert_eq!(
        std::fs::read_dir(&config.new_brain_directory)
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        authority
            .issue_proof(&admin(&directory), "proof:one", NOW)
            .err()
            .unwrap()
            .0,
        Code::Conflict
    );
    assert_eq!(
        authority
            .candidate(&service, &binding, NOW)
            .err()
            .unwrap()
            .0,
        Code::Conflict
    );
    drop(authority);
    let log = std::fs::read_to_string(&config.journal_path).unwrap();
    assert!(log.contains("Committed"));
    assert!(!log.contains(&proof.proof));
    assert!(!log.contains(WEB));
    assert!(!log.contains("issuer.key"));
}

#[test]
fn concurrent_browsers_cannot_both_reserve_or_replace_the_setup() {
    let (directory, config) = fixture();
    let authority = Arc::new(open(&config));
    let proof = authority
        .issue_proof(&admin(&directory), "proof:one", NOW)
        .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut tasks = Vec::new();
    for operation in ["claim:first", "claim:second"] {
        let authority = authority.clone();
        let barrier = barrier.clone();
        let proof = proof.proof.clone();
        tasks.push(std::thread::spawn(move || {
            barrier.wait();
            reserve(&authority, &proof, &session(operation), NOW)
        }));
    }
    barrier.wait();
    let results: Vec<_> = tasks.into_iter().map(|task| task.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| result.as_ref().err().is_some_and(|e| e.0 == Code::Conflict))
            .count(),
        1
    );
    let winner = results.into_iter().find_map(Result::ok).unwrap();
    let mut different = session(&winner.operation_id);
    different.setup_digest = p::canonical_digest(&"another-browser").unwrap();
    assert_eq!(
        reserve(&authority, &proof.proof, &different, NOW)
            .err()
            .unwrap()
            .0,
        Code::Conflict
    );
}

#[test]
fn expired_reservations_require_local_rotation_and_never_refresh_their_ttl() {
    let (directory, config) = fixture();
    let authority = open(&config);
    let operator = admin(&directory);
    let proof = authority.issue_proof(&operator, "proof:one", NOW).unwrap();
    let binding = session("claim:expired");
    reserve(&authority, &proof.proof, &binding, NOW).unwrap();
    assert_eq!(
        choose(&authority, &binding, NOW + SETUP_TTL)
            .err()
            .unwrap()
            .0,
        Code::Expired
    );
    assert_eq!(
        reserve(
            &authority,
            &proof.proof,
            &session("claim:steal"),
            NOW + SETUP_TTL
        )
        .err()
        .unwrap()
        .0,
        Code::Conflict
    );
    let replacement = authority
        .issue_proof(&operator, "proof:replacement", NOW + SETUP_TTL)
        .unwrap();
    assert_ne!(proof.proof, replacement.proof);
    assert_eq!(
        reserve(&authority, &proof.proof, &binding, NOW + SETUP_TTL)
            .err()
            .unwrap()
            .0,
        Code::Unauthenticated
    );
    reserve(
        &authority,
        &replacement.proof,
        &session("claim:replacement"),
        NOW + SETUP_TTL,
    )
    .unwrap();
}

#[test]
fn expired_proof_and_backwards_clock_have_zero_new_facts() {
    let (directory, config) = fixture();
    let authority = open(&config);
    let proof = authority
        .issue_proof(&admin(&directory), "proof:one", NOW)
        .unwrap();
    let before = std::fs::metadata(&config.journal_path).unwrap().len();
    assert_eq!(
        reserve(
            &authority,
            &proof.proof,
            &session("claim:expired"),
            NOW + PROOF_TTL
        )
        .err()
        .unwrap()
        .0,
        Code::Expired
    );
    assert_eq!(
        reserve(
            &authority,
            &proof.proof,
            &session("claim:old-clock"),
            NOW - 1
        )
        .err()
        .unwrap()
        .0,
        Code::InvalidInput
    );
    assert_eq!(
        std::fs::metadata(&config.journal_path).unwrap().len(),
        before
    );
}

#[test]
fn committed_response_loss_recovers_the_same_owner_after_restart_and_expiry() {
    let (directory, config) = fixture();
    let binding = session("claim:restart");
    let original = {
        let authority = open(&config);
        let proof = authority
            .issue_proof(&admin(&directory), "proof:one", NOW)
            .unwrap();
        reserve(&authority, &proof.proof, &binding, NOW).unwrap();
        choose(&authority, &binding, NOW).unwrap();
        commit(&authority, &binding, "account:one", NOW).unwrap()
    };
    let before = std::fs::read(&config.journal_path).unwrap();
    let authority = open(&config);
    let recovered = commit(&authority, &binding, "account:one", NOW + PROOF_TTL).unwrap();
    assert_eq!(recovered.receipt.owner, original.receipt.owner);
    assert_eq!(
        recovered.receipt.account_commitment,
        original.receipt.account_commitment
    );
    assert!(recovered.receipt.expires_at > original.receipt.expires_at);
    assert_eq!(
        commit(&authority, &binding, "account:replacement", NOW + PROOF_TTL)
            .err()
            .unwrap()
            .0,
        Code::Conflict
    );
    assert_eq!(
        commit(
            &authority,
            &session("claim:other"),
            "account:one",
            NOW + PROOF_TTL
        )
        .err()
        .unwrap()
        .0,
        Code::Conflict
    );
    drop(authority);
    assert_eq!(std::fs::read(&config.journal_path).unwrap(), before);
}

#[test]
fn data_appearing_between_selection_and_commit_blocks_without_a_signed_claim() {
    let (directory, config) = fixture();
    let authority = open(&config);
    let binding = session("claim:data-change");
    let proof = authority
        .issue_proof(&admin(&directory), "proof:one", NOW)
        .unwrap();
    reserve(&authority, &proof.proof, &binding, NOW).unwrap();
    assert_eq!(
        commit(&authority, &binding, "account", NOW)
            .err()
            .unwrap()
            .0,
        Code::Conflict
    );
    choose(&authority, &binding, NOW).unwrap();
    write_private(
        &config.new_brain_directory.join("historical-data"),
        b"fixture historical data",
    );
    let before = std::fs::metadata(&config.journal_path).unwrap().len();
    assert_eq!(
        commit(&authority, &binding, "account", NOW)
            .err()
            .unwrap()
            .0,
        Code::DataUnavailable
    );
    assert!(
        !authority
            .candidate(&authority.authenticate_service(WEB).unwrap(), &binding, NOW)
            .unwrap()
            .available
    );
    assert_eq!(
        std::fs::metadata(&config.journal_path).unwrap().len(),
        before
    );
}

#[test]
fn bootstrap_credentials_are_distinct_from_administration_and_agent_execution() {
    let (directory, config) = fixture();
    let authority = open(&config);
    for credential in [ADMIN, AGENT, "short"] {
        assert!(authority.authenticate_service(credential).is_err());
    }
    let broker = ProductHostBroker::open_with_service_manager(
        &directory.0.join("separate-admin.journal"),
        "installation:claim-fixture",
        HostCredentials::new(ADMIN.into(), Some(AGENT.into())).unwrap(),
        p::ProductTarget::WindowsX86_64,
        "fixture-v1".into(),
        SystemServiceManager,
    )
    .unwrap();
    assert!(broker.authenticate(WEB).is_err());
    assert!(broker.authenticate_agent(WEB).is_err());
    let agent = broker.authenticate_agent(AGENT).unwrap();
    assert_eq!(
        authority
            .issue_proof(&agent, "proof:agent", NOW)
            .err()
            .unwrap()
            .0,
        Code::Unauthenticated
    );
    drop(authority);
    assert_eq!(
        BootstrapAuthority::open(config.clone(), p::canonical_digest(&WEB).unwrap(), None)
            .err()
            .unwrap()
            .0,
        Code::InvalidInput
    );
}

#[test]
fn claim_journal_has_one_writer_and_rejects_torn_or_tampered_history() {
    use std::io::Write;
    let (directory, config) = fixture();
    let authority = open(&config);
    assert_eq!(
        BootstrapAuthority::open(config.clone(), p::canonical_digest(&ADMIN).unwrap(), None)
            .err()
            .unwrap()
            .0,
        Code::Conflict
    );
    authority
        .issue_proof(&admin(&directory), "proof:one", NOW)
        .unwrap();
    drop(authority);
    let original = std::fs::read(&config.journal_path).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&config.journal_path)
        .unwrap()
        .write_all(b"{\"incomplete\":")
        .unwrap();
    assert_eq!(
        BootstrapAuthority::open(config.clone(), p::canonical_digest(&ADMIN).unwrap(), None)
            .err()
            .unwrap()
            .0,
        Code::RecoveryRequired
    );
    let torn = std::fs::read(&config.journal_path).unwrap();
    assert!(torn.starts_with(&original));
    write_private(&config.journal_path, &original);
    let mut tampered = String::from_utf8(original).unwrap();
    tampered = tampered.replacen("ProofIssued", "ProofForged", 1);
    write_private(&config.journal_path, tampered.as_bytes());
    assert!(
        BootstrapAuthority::open(config.clone(), p::canonical_digest(&ADMIN).unwrap(), None)
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(&config.journal_path).unwrap(),
        tampered
    );
}

#[test]
fn foreign_installation_or_replaced_signer_cannot_rebind_existing_claims() {
    let (_directory, config) = fixture();
    drop(open(&config));
    let original = std::fs::read(&config.journal_path).unwrap();
    let mut foreign = config.clone();
    foreign.install_ref = "installation:foreign".into();
    assert_eq!(
        BootstrapAuthority::open(foreign, p::canonical_digest(&ADMIN).unwrap(), None)
            .err()
            .unwrap()
            .0,
        Code::RecoveryRequired
    );
    write_private(&config.signing_key_path, &[119; 32]);
    assert_eq!(
        BootstrapAuthority::open(config.clone(), p::canonical_digest(&ADMIN).unwrap(), None)
            .err()
            .unwrap()
            .0,
        Code::RecoveryRequired
    );
    assert_eq!(std::fs::read(&config.journal_path).unwrap(), original);
}

#[test]
fn config_and_setup_inputs_are_closed_bounded_and_paths_cannot_overlap() {
    let (directory, config) = fixture();
    let mut value = serde_json::to_value(&config).unwrap();
    value["allow_plaintext"] = true.into();
    let path = directory.0.join("config.json");
    write_private(&path, &serde_json::to_vec(&value).unwrap());
    assert!(BootstrapAuthorityConfig::load(&path).is_err());
    let mut unsafe_config = config.clone();
    unsafe_config.new_brain_directory = directory.0.clone();
    assert!(unsafe_config.validate().is_err());
    value = serde_json::to_value(&config).unwrap();
    value["journal_path"] = "relative.journal".into();
    write_private(&path, &serde_json::to_vec(&value).unwrap());
    assert!(BootstrapAuthorityConfig::load(&path).is_err());
    write_private(&path, &vec![b' '; 32 * 1024 + 1]);
    assert!(BootstrapAuthorityConfig::load(&path).is_err());
    let request = serde_json::json!({ "schema_version":1, "operation_id":"claim:fixture",
        "proof":"0".repeat(64), "setup_digest":p::canonical_digest(&"fixture").unwrap(), "owner":"owner:injected" });
    assert!(serde_json::from_value::<p::BootstrapReserveRequest>(request).is_err());
}

#[test]
fn legacy_config_without_candidate_roots_remains_new_brain_only() {
    let (directory, config) = fixture();
    let mut value = serde_json::to_value(&config).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("historical_brain_directory");
    value
        .as_object_mut()
        .unwrap()
        .remove("verified_backup_directory");
    let path = directory.0.join("config.json");
    write_private(&path, &serde_json::to_vec(&value).unwrap());
    let loaded = BootstrapAuthorityConfig::load(&path).unwrap();
    assert_eq!(loaded.candidate_roots().count(), 0);
    loaded.check_empty_target().unwrap();
}

#[test]
fn candidate_roots_are_fixed_and_cannot_overlap_managed_state() {
    let (directory, mut config) = fixture();
    config.historical_brain_directory = Some(directory.0.join("historical"));
    config.verified_backup_directory = Some(directory.0.join("backups"));
    assert!(config.validate().is_ok());

    config.verified_backup_directory = Some(config.new_brain_directory.clone());
    assert!(config.validate().is_err());

    config.verified_backup_directory = Some(directory.0.join("backups"));
    config.historical_brain_directory = Some(directory.0.join("alias"));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&directory.0, &directory.0.join("alias")).unwrap();
        assert!(config.validate().is_err());
    }
}
