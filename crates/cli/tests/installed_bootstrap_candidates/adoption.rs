//! Actual fixed Host -> data-worker transaction over a protected historical DB.
use super::*;

fn choose(f: &Fixture, daemon: &Daemon) -> (LocalBootstrapClient, p::BootstrapClaimRequestV2) {
    choose_kind(f, daemon, p::BootstrapCandidateKind::ExistingBrain)
}

fn choose_kind(
    f: &Fixture,
    daemon: &Daemon,
    kind: p::BootstrapCandidateKind,
) -> (LocalBootstrapClient, p::BootstrapClaimRequestV2) {
    let request = request();
    let web = f.reserve(daemon, &request);
    let catalog = web.candidate_catalog_v2(&request).unwrap();
    let candidate = catalog
        .candidates
        .into_iter()
        .find(|c| c.kind == kind)
        .unwrap();
    assert_eq!(candidate.status, p::BootstrapCandidateStatus::Available);
    web.select_candidate_v2(&p::BootstrapCandidateSelectionV2 {
        schema_version: p::SchemaVersion(2),
        session: request.session.clone(),
        candidate_ref: candidate.candidate_ref.clone(),
        expected_candidate_digest: candidate.candidate_digest.clone(),
    })
    .unwrap();
    (
        web,
        p::BootstrapClaimRequestV2 {
            schema_version: p::SchemaVersion(2),
            session: request.session,
            candidate_ref: candidate.candidate_ref,
            expected_candidate_digest: candidate.candidate_digest,
            account_commitment: p::canonical_digest(&"fixture original account").unwrap(),
        },
    )
}

#[test]
fn real_host_adopts_encrypted_data_before_signing_without_core_or_account() {
    let f = Fixture::new();
    let source = source::seed(&f);
    fs::write(
        source.join("artifacts").join("preserved.txt"),
        b"original artifact",
    )
    .unwrap();
    let original = fs::read(source.join("forme.db")).unwrap();
    let daemon = Daemon::start_with_configuration(&f);
    let (web, request) = choose(&f, &daemon);
    let (_, concurrent_web) = f.clients(&daemon);
    let concurrent_request = request.clone();
    let duplicate = std::thread::spawn(move || concurrent_web.adopt_data_v2(&concurrent_request));
    let result = web.adopt_data_v2(&request);
    let duplicate = duplicate.join().unwrap();
    let grant = result.unwrap();
    assert_eq!(duplicate.unwrap(), grant);
    grant.validate().unwrap();
    assert_eq!(grant.receipt.owner_ref.0, "owner:historical-cli");
    assert_eq!(grant.receipt.account_commitment, request.account_commitment);
    let once = fs::metadata(f.install.paths.host_state.join("bootstrap.journal"))
        .unwrap()
        .len();
    assert_eq!(web.adopt_data_v2(&request).unwrap(), grant);
    assert_eq!(
        fs::metadata(f.install.paths.host_state.join("bootstrap.journal"))
            .unwrap()
            .len(),
        once
    );
    let query = p::BootstrapClaimQueryV2 {
        schema_version: p::SchemaVersion(2),
        session: request.session.clone(),
    };
    assert_eq!(web.claim_activation_v2(&query).unwrap(), grant);
    let mut changed = request.clone();
    changed.account_commitment = p::canonical_digest(&"substituted account").unwrap();
    assert_eq!(
        web.adopt_data_v2(&changed).unwrap_err().0,
        p::BootstrapProblemCode::Conflict
    );
    let worker_root = fs::read_dir(f.install.paths.host_state.join("bootstrap-data"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let intent: p::BootstrapDataIntentV2 =
        serde_json::from_slice(&fs::read(worker_root.join("intent.json")).unwrap()).unwrap();
    let reference = intent.target_ref().unwrap();
    let root = f.install.paths.brain.join("bootstrap-data").join(reference);
    assert!(root.join("forme.db").is_file() && root.join("forme.content-keys").is_dir());
    assert!(!f.install.paths.brain.join("forme.db").exists());
    assert_eq!(fs::read(source.join("forme.db")).unwrap(), original);
    assert_eq!(
        fs::read(f.install.paths.brain.join("artifacts/preserved.txt")).unwrap(),
        b"original artifact"
    );
    assert!(!f.install.paths.web_auth.join("identity.sqlite").exists());
    assert!(!f.install.paths.config.join("core/core.json").exists());
    assert!(!f
        .install
        .paths
        .brain
        .join("runtime/core-config-ack.json")
        .exists());
    drop(daemon);
    let journal = fs::read_to_string(f.install.paths.host_state.join("bootstrap.journal")).unwrap();
    let facts: Vec<_> = journal
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["fact"]["kind"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        &facts[facts.len() - 5..],
        &[
            "ClaimDataIntentV2",
            "ClaimDataPrepared",
            "ClaimCommittedV2",
            "ClaimDataBindingConfirmedV2",
            "ClaimActivatedV2"
        ]
    );
    let daemon = Daemon::start_with_configuration(&f);
    let (_, web) = f.clients(&daemon);
    assert_eq!(web.claim_activation_v2(&query).unwrap(), grant);
    assert_eq!(
        web.claim_state_v2(&query).unwrap().phase,
        p::BootstrapClaimPhaseV2::Activated
    );
}

#[test]
fn real_host_restores_verified_backup_preserving_source_owner_and_original_files() {
    let f = Fixture::new();
    let source = source::seed(&f);
    let original = fs::read(source.join("forme.db")).unwrap();
    let backup = f.install.paths.backups.join("candidates/verified-snapshot");
    fs::create_dir_all(&backup).unwrap();
    source::copy_tree(
        &source.join("forme.content-keys"),
        &backup.join("forme.content-keys"),
    );
    fs::create_dir(backup.join("artifacts")).unwrap();
    let core_path = source.join("core.json");
    let mut core: serde_json::Value =
        serde_json::from_slice(&fs::read(&core_path).unwrap()).unwrap();
    core["brain"]["store_path"] = serde_json::to_value(backup.join("forme.db")).unwrap();
    core["brain"]["artifact_root"] = serde_json::to_value(backup.join("artifacts")).unwrap();
    fs::write(backup.join("core.json"), serde_json::to_vec(&core).unwrap()).unwrap();
    let data = forme_gateway::offline_data::OfflineCoreMaintenance::open(&core_path, None).unwrap();
    data.backup(&backup.join("backup.db")).unwrap();
    drop(data);
    let before = fs::read(backup.join("backup.db")).unwrap();
    let daemon = Daemon::start_with_configuration(&f);
    let (web, request) = choose_kind(&f, &daemon, p::BootstrapCandidateKind::VerifiedBackup);
    let grant = web.adopt_data_v2(&request).unwrap();
    assert_eq!(
        grant.receipt.candidate_kind,
        p::BootstrapCandidateKind::VerifiedBackup
    );
    assert_eq!(grant.receipt.owner_ref.0, "owner:historical-cli");
    assert_eq!(fs::read(backup.join("backup.db")).unwrap(), before);
    assert_eq!(fs::read(source.join("forme.db")).unwrap(), original);
    assert!(!backup.join("forme.db").exists());
    assert!(!f.install.paths.config.join("core/core.json").exists());
    assert!(!f.install.paths.web_auth.join("identity.sqlite").exists());
}

#[test]
fn real_host_restart_after_config_cas_requires_recovery_without_recopying_or_signing() {
    let f = Fixture::new();
    let source = source::seed(&f);
    let original = fs::read(source.join("forme.db")).unwrap();
    let daemon = Daemon::start_with_configuration(&f);
    let (web, request) = choose(&f, &daemon);
    let grant = web.adopt_data_v2(&request).unwrap();
    drop(daemon);
    let journal_path = f.install.paths.host_state.join("bootstrap.journal");
    let completed = fs::read_to_string(&journal_path).unwrap();
    let query = p::BootstrapClaimQueryV2 {
        schema_version: p::SchemaVersion(2),
        session: request.session.clone(),
    };
    for stop_at in ["ClaimCommittedV2", "ClaimDataBindingConfirmedV2"] {
        // Keep an exact, hash-valid durable prefix of this actual successful
        // transaction. Config/target stay committed: this models loss before
        // confirmation/activation append, never damage or repair of a journal.
        let mut interrupted = String::new();
        for line in completed.lines() {
            interrupted.push_str(line);
            interrupted.push('\n');
            if serde_json::from_str::<serde_json::Value>(line).unwrap()["fact"]["kind"] == stop_at {
                break;
            }
        }
        fs::write(&journal_path, interrupted.as_bytes()).unwrap();
        let daemon = Daemon::start_with_configuration(&f);
        let (_, web) = f.clients(&daemon);
        let view = web.claim_state_v2(&query).unwrap();
        assert_eq!(view.phase, p::BootstrapClaimPhaseV2::RecoveryRequired);
        assert_eq!(view.receipt, Some(grant.receipt.clone()));
        assert!(view.activation.is_none());
        assert_eq!(
            web.claim_activation_v2(&query).unwrap_err().0,
            p::BootstrapProblemCode::RecoveryRequired
        );
        assert_eq!(
            web.adopt_data_v2(&request).unwrap_err().0,
            p::BootstrapProblemCode::RecoveryRequired
        );
        assert!(!f.install.paths.config.join("core/core.json").exists());
        drop(daemon);
        assert_eq!(fs::read(&journal_path).unwrap(), interrupted.as_bytes());
        assert_eq!(fs::read(source.join("forme.db")).unwrap(), original);
    }
    // Replaying the complete original durable history may expose only its
    // original signature. It must not need a second worker transaction.
    fs::write(&journal_path, completed.as_bytes()).unwrap();
    let daemon = Daemon::start_with_configuration(&f);
    let (_, web) = f.clients(&daemon);
    assert_eq!(web.claim_activation_v2(&query).unwrap(), grant);
}
