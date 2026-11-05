//! Real installer layout, hostd RPC and fixed hostctl child. No native services.
#[path = "installed_bootstrap_candidates/adoption.rs"]
mod adoption;
#[path = "installed_bootstrap_candidates/data_worker.rs"]
mod data_worker;
#[path = "installed_bootstrap_candidates/source.rs"]
mod source;
use forme_cli::hostctl_install::{prepare, PrepareRequest};
use forme_gateway::web::bootstrap::LocalBootstrapClient;
use forme_protocol as p;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Fixture {
    root: PathBuf,
    manifest_path: PathBuf,
    install: p::InstallationManifest,
    setup: serde_json::Value,
    admin: String,
}

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forme-bootstrap-candidates-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let report = prepare(PrepareRequest {
            install_root: root.join("install"),
            state_root: root.join("state"),
            config_root: root.join("config"),
            log_root: root.join("logs"),
            ipc_root: root.join("ipc"),
            target: if cfg!(windows) {
                p::ProductTarget::WindowsX86_64
            } else {
                p::ProductTarget::Ubuntu2404X86_64
            },
            release_id: "fixture-1".into(),
            release_digest: p::sha256_content_digest(b"fixture release"),
        })
        .unwrap();
        let install: p::InstallationManifest =
            serde_json::from_slice(&fs::read(&report.manifest_path).unwrap()).unwrap();
        let setup =
            serde_json::from_slice(&fs::read(&report.bootstrap_config_path).unwrap()).unwrap();
        let host: serde_json::Value =
            serde_json::from_slice(&fs::read(&report.host_config_path).unwrap()).unwrap();
        let admin =
            fs::read_to_string(host["maintenance_credential_path"].as_str().unwrap()).unwrap();
        let program = install.paths.releases.join("bin").join(if cfg!(windows) {
            "forme-hostctl.exe"
        } else {
            "forme-hostctl"
        });
        fs::create_dir_all(program.parent().unwrap()).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_forme-hostctl"), program).unwrap();
        Self {
            root,
            manifest_path: report.manifest_path,
            install,
            setup,
            admin,
        }
    }

    fn worker(&self, args: &[String], body: &[u8]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forme-hostctl"));
        command
            .env_clear()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        platform_environment(&mut command, &self.root);
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(body).unwrap();
        child.wait_with_output().unwrap()
    }

    fn clients(&self, daemon: &Daemon) -> (LocalBootstrapClient, LocalBootstrapClient) {
        let host: serde_json::Value =
            serde_json::from_slice(&fs::read(self.install.paths.config.join("host.json")).unwrap())
                .unwrap();
        (
            LocalBootstrapClient::connect(
                daemon.address,
                Path::new(host["maintenance_credential_path"].as_str().unwrap()),
            )
            .unwrap(),
            LocalBootstrapClient::connect(
                daemon.address,
                Path::new(self.setup["web_credential_path"].as_str().unwrap()),
            )
            .unwrap(),
        )
    }

    fn reserve(
        &self,
        daemon: &Daemon,
        request: &p::BootstrapCandidateCatalogRequestV2,
    ) -> LocalBootstrapClient {
        let (admin, web) = self.clients(daemon);
        let proof = admin.issue_proof("proof:real-candidates").unwrap();
        web.reserve(&p::BootstrapReserveRequest {
            schema_version: p::SchemaVersion(1),
            operation_id: request.session.operation_id.clone(),
            proof: proof.proof,
            setup_digest: request.session.setup_digest.clone(),
        })
        .unwrap();
        web
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let actual = self.root.canonicalize().unwrap();
        assert_eq!(
            actual.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        assert!(actual
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("forme-bootstrap-candidates-"));
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn platform_environment(command: &mut Command, root: &Path) {
    command
        .env("TEMP", root)
        .env("TMP", root)
        .env("TMPDIR", root);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        if let Some(system) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system);
        }
    }
}

struct Daemon {
    child: Child,
    address: std::net::SocketAddr,
}
impl Daemon {
    fn start(f: &Fixture) -> Self {
        Self::start_inner(f, false)
    }

    fn start_with_configuration(f: &Fixture) -> Self {
        Self::start_inner(f, true)
    }

    fn start_inner(f: &Fixture, configured_host: bool) -> Self {
        let name = if cfg!(windows) {
            "forme-hostd.exe"
        } else {
            "forme-hostd"
        };
        let program = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(name);
        assert!(
            program.is_file(),
            "build workspace product binaries before this integration test"
        );
        let mut command = Command::new(program);
        command.env_clear();
        platform_environment(&mut command, &f.root);
        if configured_host {
            let path = f.install.paths.config.join("host.json");
            let mut config: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            // This isolated setup test has no AgentExecution backend. Keep it
            // unavailable rather than installing a native worker/service.
            config["worker_config_path"] = serde_json::Value::Null;
            let sockets: Vec<_> = (0..3)
                .map(|_| std::net::TcpListener::bind("127.0.0.1:0").unwrap())
                .collect();
            for (field, socket) in ["host_bind", "agent_execution_bind", "configuration_bind"]
                .into_iter()
                .zip(&sockets)
            {
                config[field] = serde_json::json!(socket.local_addr().unwrap());
            }
            fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
            command.args(["--development-config"]).arg(path);
        }
        let mut child = command
            .env("FORME_HOST_BIND", "127.0.0.1:0")
            .env("FORME_AGENT_EXECUTION_BIND", "127.0.0.1:0")
            .env(
                "FORME_HOST_JOURNAL_PATH",
                f.install.paths.host_state.join("host.journal"),
            )
            .env(
                "FORME_BOOTSTRAP_CONFIG_PATH",
                f.install.paths.config.join("bootstrap.json"),
            )
            .env("FORME_HOST_TOKEN", f.admin.trim())
            .env(
                "FORME_AGENT_EXECUTION_TOKEN",
                "fixture-agent-credential-0123456789abcdef",
            )
            .env("FORME_INSTALL_ID", &f.install.install_id)
            .env("FORME_ACTIVE_RELEASE", "fixture-1")
            .env(
                "FORME_PRODUCT_TARGET",
                if cfg!(windows) {
                    "windows_x86_64"
                } else {
                    "ubuntu_2404_x86_64"
                },
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(if configured_host {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, receive) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut lines = Vec::new();
            for _ in 0..if configured_host { 3 } else { 2 } {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                lines.push(line);
            }
            let _ = send.send(lines);
        });
        let mut daemon = Self {
            child,
            address: "127.0.0.1:1".parse().unwrap(),
        };
        let lines = receive
            .recv_timeout(Duration::from_secs(15))
            .expect("fixture Host did not become available");
        reader.join().unwrap();
        daemon.address = lines[0]
            .trim()
            .strip_prefix("FORME_HOST_URL=http://")
            .unwrap()
            .parse()
            .unwrap();
        daemon
    }
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn request() -> p::BootstrapCandidateCatalogRequestV2 {
    p::BootstrapCandidateCatalogRequestV2 {
        schema_version: p::SchemaVersion(2),
        session: p::BootstrapSessionBindingV2 {
            schema_version: p::SchemaVersion(2),
            operation_id: "claim:real-candidates".into(),
            setup_digest: p::canonical_digest(&"fixture setup").unwrap(),
        },
    }
}

#[test]
fn fixed_child_validates_candidates_without_creating_claim_brain_or_runtime() {
    let f = Fixture::new();
    let source = f
        .install
        .paths
        .backups
        .join("historical-brains/unverified-source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("private-filename"), b"untrusted input").unwrap();
    let mut body = serde_json::to_vec(&request()).unwrap();
    body.push(b'\n');
    let args = vec![
        "maintenance-bootstrap-candidates".into(),
        "--installation-manifest".into(),
        f.manifest_path.to_str().unwrap().into(),
        "--verification-epoch".into(),
        "5".into(),
    ];
    let output = f.worker(&args, &body);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let catalog: p::BootstrapCandidateCatalogV2 = serde_json::from_slice(&output.stdout).unwrap();
    catalog.validate().unwrap();
    assert_eq!(catalog.verification_epoch, 5);
    assert_eq!(catalog.candidates.len(), 2);
    assert_eq!(
        catalog.candidates[0].status,
        p::BootstrapCandidateStatus::Available
    );
    assert_eq!(
        catalog.candidates[1].status,
        p::BootstrapCandidateStatus::Unverifiable
    );
    assert!(catalog.candidates[1].owner_ref.is_none());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("unverified-source"));
    assert!(!f
        .install
        .paths
        .host_state
        .join("bootstrap.journal")
        .exists());
    assert!(!f.install.paths.brain.join("forme.db").exists());
    assert!(!f.install.paths.config.join("core/core.json").exists());
    for extra in 0..4 {
        fs::create_dir(source.parent().unwrap().join(format!("extra-{extra}"))).unwrap();
    }
    let output = f.worker(&args, &body);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn real_host_catalog_owns_epoch_authentication_journal_and_preserves_new_brain_compatibility() {
    let f = Fixture::new();
    let daemon = Daemon::start(&f);
    // Use the actual installer maintenance credential, never the signing key.
    let maintenance = f.root.join("config/host.json");
    let host: serde_json::Value = serde_json::from_slice(&fs::read(maintenance).unwrap()).unwrap();
    let admin = LocalBootstrapClient::connect(
        daemon.address,
        Path::new(host["maintenance_credential_path"].as_str().unwrap()),
    )
    .unwrap();
    let web = LocalBootstrapClient::connect(
        daemon.address,
        Path::new(f.setup["web_credential_path"].as_str().unwrap()),
    )
    .unwrap();
    let request = request();
    assert_eq!(
        web.candidate_catalog_v2(&request).unwrap_err().0,
        p::BootstrapProblemCode::Unauthenticated
    );
    let proof = admin.issue_proof("proof:real-candidates").unwrap();
    web.reserve(&p::BootstrapReserveRequest {
        schema_version: p::SchemaVersion(1),
        operation_id: request.session.operation_id.clone(),
        proof: proof.proof,
        setup_digest: request.session.setup_digest.clone(),
    })
    .unwrap();
    assert_eq!(
        admin.candidate_catalog_v2(&request).unwrap_err().0,
        p::BootstrapProblemCode::Unauthenticated
    );
    let mut observations = std::thread::scope(|scope| {
        let first = scope.spawn(|| web.candidate_catalog_v2(&request).unwrap());
        let second = scope.spawn(|| web.candidate_catalog_v2(&request).unwrap());
        vec![first.join().unwrap(), second.join().unwrap()]
    });
    observations.sort_by_key(|catalog| catalog.verification_epoch);
    assert_eq!(observations[0].verification_epoch, 1);
    let second = observations.pop().unwrap();
    assert_eq!(second.verification_epoch, 2);
    let selected = p::BootstrapCandidateSelectionV2 {
        schema_version: p::SchemaVersion(2),
        session: request.session.clone(),
        candidate_ref: second.candidates[0].candidate_ref.clone(),
        expected_candidate_digest: second.candidates[0].candidate_digest.clone(),
    };
    assert_eq!(
        web.select_candidate_v2(&selected).unwrap_err().0,
        p::BootstrapProblemCode::Conflict
    );
    let view = web
        .claim_state_v2(&p::BootstrapClaimQueryV2 {
            schema_version: p::SchemaVersion(2),
            session: request.session.clone(),
        })
        .unwrap();
    assert_eq!(view.phase, p::BootstrapClaimPhaseV2::Observed);
    assert!(view.activation.is_none());
    let session = p::BootstrapSessionBinding {
        schema_version: p::SchemaVersion(1),
        operation_id: request.session.operation_id.clone(),
        setup_digest: request.session.setup_digest.clone(),
    };
    web.select(&p::BootstrapDataChoice {
        schema_version: p::SchemaVersion(1),
        session: session.clone(),
        candidate_ref: second.candidates[0].candidate_ref.clone(),
    })
    .unwrap();
    web.commit(&p::BootstrapCommitRequest {
        schema_version: p::SchemaVersion(1),
        session,
        candidate_ref: second.candidates[0].candidate_ref.clone(),
        account_commitment: p::canonical_digest(&"fixture account").unwrap(),
    })
    .unwrap();
    assert_eq!(
        web.candidate_catalog_v2(&request).unwrap_err().0,
        p::BootstrapProblemCode::Conflict
    );
    assert!(!f.install.paths.brain.join("forme.db").exists());
}

#[test]
fn real_host_rechecks_source_before_selection_and_recovers_selection_without_activation() {
    for drift in [false, true] {
        let f = Fixture::new();
        let source = source::seed(&f);
        let before = fs::read(source.join("forme.db")).unwrap();
        let daemon = Daemon::start(&f);
        let request = request();
        let web = f.reserve(&daemon, &request);
        let original = web.candidate_catalog_v2(&request).unwrap();
        let selected = original
            .candidates
            .iter()
            .find(|candidate| candidate.kind == p::BootstrapCandidateKind::ExistingBrain)
            .unwrap();
        assert_eq!(selected.status, p::BootstrapCandidateStatus::Available);
        assert_eq!(
            selected.owner_ref,
            Some(p::OwnerPrincipalRef("owner:historical-cli".into()))
        );
        let selection = p::BootstrapCandidateSelectionV2 {
            schema_version: p::SchemaVersion(2),
            session: request.session.clone(),
            candidate_ref: selected.candidate_ref.clone(),
            expected_candidate_digest: selected.candidate_digest.clone(),
        };
        if drift {
            let core_path = source.join("core.json");
            let mut core: serde_json::Value =
                serde_json::from_slice(&fs::read(&core_path).unwrap()).unwrap();
            core["revision"] = serde_json::json!(2);
            fs::write(core_path, serde_json::to_vec(&core).unwrap()).unwrap();
        }
        let journal_path = f.install.paths.host_state.join("bootstrap.journal");
        let journal_before = fs::metadata(&journal_path).unwrap().len();
        if drift {
            assert_eq!(
                web.select_candidate_v2(&selection).unwrap_err().0,
                p::BootstrapProblemCode::Conflict
            );
            assert_eq!(fs::metadata(&journal_path).unwrap().len(), journal_before);
        } else {
            assert_eq!(web.select_candidate_v2(&selection).unwrap(), *selected);
            let once = fs::metadata(&journal_path).unwrap().len();
            assert!(once > journal_before);
            assert_eq!(web.select_candidate_v2(&selection).unwrap(), *selected);
            assert_eq!(fs::metadata(&journal_path).unwrap().len(), once);
        }
        drop(daemon);
        let recovered_journal = fs::read(&journal_path).unwrap();
        let daemon = Daemon::start(&f);
        let (_, web) = f.clients(&daemon);
        let state = web
            .claim_state_v2(&p::BootstrapClaimQueryV2 {
                schema_version: p::SchemaVersion(2),
                session: request.session.clone(),
            })
            .unwrap();
        assert_eq!(
            state.phase,
            if drift {
                p::BootstrapClaimPhaseV2::Observed
            } else {
                p::BootstrapClaimPhaseV2::Selected
            }
        );
        assert!(state.receipt.is_none() && state.activation.is_none());
        if drift {
            let catalog = web.candidate_catalog_v2(&request).unwrap();
            assert_eq!(catalog.verification_epoch, original.verification_epoch + 1);
            assert_ne!(
                catalog.candidates[1].candidate_digest,
                selection.expected_candidate_digest
            );
        } else {
            let unchanged = fs::metadata(&journal_path).unwrap().len();
            assert_eq!(web.candidate_catalog_v2(&request).unwrap(), original);
            assert_eq!(fs::metadata(&journal_path).unwrap().len(), unchanged);
            assert_eq!(web.select_candidate_v2(&selection).unwrap(), *selected);
        }
        assert_eq!(fs::read(source.join("forme.db")).unwrap(), before);
        assert!(fs::read_dir(&f.install.paths.brain)
            .unwrap()
            .next()
            .is_none());
        assert!(!f.install.paths.web_auth.join("identity.sqlite").exists());
        assert!(!f.install.paths.config.join("core/core.json").exists());
        drop(daemon);
        if !drift {
            assert_eq!(fs::read(&journal_path).unwrap(), recovered_journal);
        }
    }
}
