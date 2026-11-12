//! Explicit native components; these do not establish installed service identity.
use super::super::{product_config::tests as config_tests, ManagedMcpProgramSource};
use super::*;
use serde_json::{json, Value};

struct Fixture {
    _temp: tempfile::TempDir,
    config: Arc<Mutex<super::super::ProductConfigStore>>,
    programs: Arc<InstalledManagedMcpPrograms>,
    request: p::WorkerDispatchRequest,
    settings: p::ProductMcpServerConfig,
    registration: p::ManagedMcpProgramOperation,
    runtime: PathBuf,
    _listener: std::net::TcpListener,
}
impl Fixture {
    fn new(mode: &str, timeout: u64) -> Self {
        Self::create(mode, timeout, true)
    }
    fn create(mode: &str, timeout: u64, staged: bool) -> Self {
        let (temp, _, config) = config_tests::fixture();
        let config = Arc::new(config);
        let root = temp.path().join("programs");
        fs::create_dir(&root).unwrap();
        let program = root.join("server.exe");
        let fixture =
            std::env::var_os("FORME_MCP_STDIO_FIXTURE").expect("explicit native fixture path");
        fs::copy(fixture, &program).unwrap();
        let private = temp.path().join("private.fixture");
        fs::write(&private, b"private-fixture-marker").unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let source = ManagedMcpProgramSource {
            schema_version: p::SchemaVersion(1),
            source_ref: "source:native".into(),
            scope: p::Scope("workspace:default".into()),
            sandbox_profile_ref: p::MANAGED_MCP_PROGRAM_PROFILE.into(),
            expected_program_digest: super::super::file_digest(&program).unwrap(),
            program,
            args: vec![
                mode.into(),
                private.to_string_lossy().into(),
                listener.local_addr().unwrap().to_string(),
            ],
        };
        fs::write(
            root.join("sources.json"),
            serde_json::to_vec(&json!({"schema_version":1,"sources":[source]})).unwrap(),
        )
        .unwrap();
        let programs = Arc::new(
            InstalledManagedMcpPrograms::new(
                temp.path().join("programs.jsonl"),
                "install:config-test".into(),
                p::AuthorityEpoch(1),
                config.clone(),
                root,
                vec![p::Scope("workspace:default".into())],
                vec![private],
            )
            .unwrap(),
        );
        let registration = programs
            .with_registry(|registry| {
                let catalog = registry.catalog()?;
                registry.apply(
                    p::ManagedMcpProgramCommand {
                        schema_version: p::SchemaVersion(1),
                        installation_ref: catalog.installation_ref,
                        owner_ref: catalog.owner_ref,
                        authority_epoch: catalog.authority_epoch,
                        operation_id: "register:native".into(),
                        expected_revision: catalog.revision,
                        requested_deadline: recovery_timestamp() + 30_000,
                        action: p::ManagedMcpProgramAction::Register {
                            source_ref: catalog.choices[0].source_ref.clone(),
                            source_digest: catalog.choices[0].source_digest.clone(),
                        },
                    },
                    recovery_timestamp(),
                )
            })
            .unwrap();
        let settings = p::ProductMcpServerConfig {
            schema_version: p::SchemaVersion(1),
            server_ref: "fixture:native".into(),
            program_ref: registration.program.program_ref.clone(),
            scope: p::Scope("workspace:default".into()),
            enabled: true,
            trust: p::ProductSkillTrust::OwnerInput,
            timeout: p::DurationMs(timeout),
            output_budget: 131_072,
            allowlist: p::ProductMcpAllowlist {
                schema_version: p::SchemaVersion(1),
                tools: vec!["fixture.inspect".into()],
                resources: vec!["fixture://methods".into()],
            },
        };
        if staged {
            config_tests::stage_mcp_fixture(&mut config.lock().unwrap(), settings.clone());
        }
        let request = request(&registration, &settings);
        let runtime = temp.path().join("runtime");
        Self {
            _temp: temp,
            config,
            programs,
            request,
            settings,
            registration,
            runtime,
            _listener: listener,
        }
    }
    fn launch(&self) -> ManagedMcpIsolation {
        let isolation =
            ManagedMcpIsolation::open(self.runtime.clone(), self.programs.clone()).unwrap();
        self.programs
            .with_discovery(
                &self.request,
                self.request.permit.issued_at,
                Instant::now(),
                |admission| isolation.admit_authorized(admission, self.request.permit.issued_at),
            )
            .unwrap();
        isolation
    }
    fn revoke(&self) {
        self.programs
            .with_registry(|registry| {
                registry.apply(
                    p::ManagedMcpProgramCommand {
                        schema_version: p::SchemaVersion(1),
                        installation_ref: "install:config-test".into(),
                        owner_ref: p::OwnerPrincipalRef("owner:test".into()),
                        authority_epoch: p::AuthorityEpoch(1),
                        operation_id: "revoke:native".into(),
                        expected_revision: self.registration.revision,
                        requested_deadline: recovery_timestamp() + 30_000,
                        action: p::ManagedMcpProgramAction::Revoke {
                            program_ref: self.registration.program.program_ref.clone(),
                            registration_digest: self
                                .registration
                                .program
                                .registration_digest
                                .clone(),
                        },
                    },
                    recovery_timestamp(),
                )
            })
            .unwrap();
    }
}
fn request(
    registration: &p::ManagedMcpProgramOperation,
    settings: &p::ProductMcpServerConfig,
) -> p::WorkerDispatchRequest {
    use crate::{DefaultExecutionPlanner, ExecutionPlanner, OutputBudget};
    let now = recovery_timestamp();
    let spec = p::ManagedMcpDiscoverySpec {
        schema_version: p::SchemaVersion(1),
        server: p::McpServerRef(settings.server_ref.clone()),
        program_ref: settings.program_ref.clone(),
        registration_digest: registration.program.registration_digest.clone(),
        settings_digest: p::managed_mcp_settings_digest(settings).unwrap(),
        timeout: settings.timeout,
        output_budget_bytes: settings.output_budget,
    };
    let intent = p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("action:native-discovery".into()),
        source: p::Source::OwnerControl,
        goal: p::GoalRef("goal:native".into()),
        backend_hint: p::BackendKind::Mcp,
        capability_ref: p::CapabilityRef(format!("mcp-discovery:{}", settings.server_ref)),
        action_type: p::ActionType::Execute,
        scope: settings.scope.clone(),
        risk_hint: p::Risk::High,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::McpDiscovery(spec),
        requested_permissions: vec![p::PermissionRef("mcp:discovery".into())],
        requested_at: now,
        estimated_output_bytes: settings.output_budget,
        estimated_duration: settings.timeout,
    };
    let plan = DefaultExecutionPlanner::new(
        OutputBudget::strict(settings.output_budget * 2),
        p::DurationMs(settings.timeout.0 + 1_000),
    )
    .plan(&intent)
    .unwrap();
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: "job:native-discovery".into(),
        owner_ref: p::VerifiedPrincipal("owner:test".into()),
        action_ref: intent.intent_id.clone(),
        affected_resources: plan.affected_resources.clone(),
        execution_plan_ref: "plan:native-discovery".into(),
        plan_digest: plan.digest.clone(),
        authority_epoch: p::AuthorityEpoch(1),
        permit_ref: "permit:native-discovery".into(),
        sandbox_profile_ref: p::MANAGED_MCP_PROGRAM_PROFILE.into(),
        workspace_scope: plan.scope.clone(),
        deadline: now + plan.timeout.0 as i64 + 1000,
        nonce: p::Nonce("nonce:native-discovery".into()),
        cancel_ref: "cancel:native".into(),
    };
    let permit = p::WorkerPermit {
        schema_version: p::SchemaVersion(1),
        permit_ref: job.permit_ref.clone(),
        job_id: job.job_id.clone(),
        owner_ref: job.owner_ref.clone(),
        action_ref: job.action_ref.clone(),
        affected_resources: job.affected_resources.clone(),
        plan_digest: job.plan_digest.clone(),
        authority_epoch: job.authority_epoch,
        workspace_scope: job.workspace_scope.clone(),
        sandbox_profile_ref: job.sandbox_profile_ref.clone(),
        deadline: job.deadline,
        nonce: job.nonce.clone(),
        issued_at: now,
    };
    let execution = p::WorkerExecutionSpec {
        schema_version: p::SchemaVersion(1),
        plan_digest: plan.digest,
        intent,
        backend: plan.backend,
        scope: plan.scope,
        affected_resources: plan.affected_resources,
        normalized_action_digest: plan.normalized_action_digest,
        timeout: plan.timeout,
        output_budget_bytes: plan.budget.max_bytes,
        desktop_session: None,
    };
    p::WorkerDispatchRequest {
        schema_version: p::SchemaVersion(1),
        job,
        permit,
        execution: Some(execution),
    }
}
fn worker_dir(isolation: &ManagedMcpIsolation, job: &p::WorkerJob) -> PathBuf {
    isolation.children.lock().unwrap()[&job.job_id].dir.clone()
}
fn wait_file(path: &Path) {
    let until = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(
            Instant::now() < until,
            "fixture did not reach observed boundary: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn wait_receipt(isolation: &ManagedMcpIsolation, job: &p::WorkerJob) -> p::WorkerReceipt {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(receipt) = isolation.observe(job, recovery_timestamp()).unwrap() {
            return receipt;
        }
        assert!(Instant::now() < until, "original worker did not settle");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn methods(dir: &Path) -> Vec<String> {
    fs::read_to_string(dir.join("workspace/requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap()["method"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}
fn seal(isolation: &ManagedMcpIsolation, job: &p::WorkerJob) {
    isolation.result_sealed(job, recovery_timestamp()).unwrap();
    assert!(isolation.children.lock().unwrap().is_empty());
    isolation
        .journal
        .verify_startup_recovery_complete()
        .unwrap();
    isolation.with_quiescent_resources(&mut || Ok(())).unwrap();
}

#[test]
#[ignore = "requires explicit compiled native MCP fixture; not installed acceptance"]
fn native_managed_mcp_discovery_and_host_result_seal() {
    let fixture = Fixture::new("normal", 10_000);
    let isolation = fixture.launch();
    let job = &fixture.request.job;
    let dir = worker_dir(&isolation, job);
    let receipt = wait_receipt(&isolation, job);
    assert_eq!(receipt.observed_outcome, p::WorkerOutcome::Completed);
    assert_eq!(
        methods(&dir),
        [
            "initialize",
            "notifications/initialized",
            "tools/list",
            "tools/list",
            "resources/list"
        ]
    );
    let p::ActionParameters::McpDiscovery(spec) = &fixture
        .request
        .execution
        .as_ref()
        .unwrap()
        .intent
        .parameters
    else {
        unreachable!()
    };
    let bytes = fs::read(isolation.result_path(job).unwrap().unwrap()).unwrap();
    let catalog = p::ManagedMcpDiscoveryCatalogV1::from_slice(&bytes, spec).unwrap();
    let probe: Value = serde_json::from_str(&catalog.tools[0].description).unwrap();
    assert!(
        probe
            .as_object()
            .unwrap()
            .values()
            .all(|value| value == true),
        "{probe}"
    );
    assert!(results::verify_cleanup_allowed(&dir).is_err());
    // Real Host sealing is tested below with a separately admitted original job.
    seal(&isolation, job);
    drop(isolation);
    host_seal(&fixture);
}
fn host_seal(fixture: &Fixture) {
    use super::super::{HostCredentials, IsolatedExecutorHost, WorkerExecutorHost};
    let credentials = HostCredentials::new("m".repeat(32), Some("a".repeat(32))).unwrap();
    let broker = super::super::ProductHostBroker::open_with_service_manager(
        &fixture._temp.path().join("host.jsonl"),
        "install:config-test",
        credentials,
        p::ProductTarget::WindowsX86_64,
        "fixture-release".into(),
        super::super::SystemServiceManager,
    )
    .unwrap();
    let auth = broker.authenticate_agent(&"a".repeat(32)).unwrap();
    let mut req = request(&fixture.registration, &fixture.settings);
    req.job.job_id = "job:host-native".into();
    req.permit.job_id = req.job.job_id.clone();
    let host = WorkerExecutorHost::open(
        &fixture._temp.path().join("worker.jsonl"),
        p::canonical_digest(&"a".repeat(32)).unwrap(),
        p::AuthorityEpoch(1),
        [req.job.workspace_scope.clone()].into_iter().collect(),
    )
    .unwrap()
    .with_managed_mcp_programs(fixture.programs.clone(), fixture.runtime.join("host"))
    .unwrap();
    let result = host
        .dispatch_with_execution(
            &auth,
            req.job.clone(),
            req.permit.clone(),
            req.execution.clone(),
            req.permit.issued_at,
        )
        .unwrap();
    assert_eq!(result.state, p::WorkerState::Running);
    let until = Instant::now() + Duration::from_secs(10);
    let terminal = loop {
        let observed = host
            .probe(&auth, &req.job.job_id, recovery_timestamp())
            .unwrap();
        if observed.state != p::WorkerState::Running {
            break observed;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(terminal.state, p::WorkerState::Completed);
    let manifest = host
        .result_manifest(&auth, &req.job.job_id, recovery_timestamp())
        .unwrap();
    assert_eq!(manifest.status, p::WorkerResultStatus::Available);
    assert!(manifest.bytes > 0);
    let chunk = host
        .read_result(
            &auth,
            p::WorkerResultRead {
                schema_version: p::SchemaVersion(1),
                binding: manifest.binding.clone(),
                manifest_digest: manifest.manifest_digest.clone(),
                offset: 0,
                max_bytes: 131_072,
            },
            recovery_timestamp(),
        )
        .unwrap();
    chunk.validate().unwrap();
    assert!(chunk.finished);
    assert_eq!(
        Some(p::sha256_content_digest(&chunk.bytes)),
        manifest.content_digest
    );
    let p::ActionParameters::McpDiscovery(spec) =
        &req.execution.as_ref().unwrap().intent.parameters
    else {
        unreachable!()
    };
    p::ManagedMcpDiscoveryCatalogV1::from_slice(&chunk.bytes, spec).unwrap();
    let envelopes: Vec<_> = fs::read_dir(fixture._temp.path().join("worker.results"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "bin"))
        .collect();
    assert_eq!(envelopes.len(), 1);
    let encrypted = fs::read(&envelopes[0]).unwrap();
    assert!(encrypted.starts_with(b"FRMRES01"));
    assert!(!encrypted
        .windows(b"fixture.inspect".len())
        .any(|bytes| bytes == b"fixture.inspect"));
    // The separate default adapter is intentionally Unavailable, so this
    // component checks the MCP run directories rather than asserting all
    // unrelated worker resources have an observation capability.
    assert!(fs::read_dir(fixture.runtime.join("host"))
        .unwrap()
        .all(|entry| !entry.unwrap().path().is_dir()));

    assert_eq!(
        host.dispatch_with_execution(
            &auth,
            req.job,
            req.permit,
            req.execution,
            recovery_timestamp()
        )
        .unwrap()
        .receipt_ref,
        terminal.receipt_ref
    );
}

#[test]
#[ignore = "requires explicit compiled native MCP fixture; not installed acceptance"]
fn native_managed_mcp_hostile_output_never_publishes_partial_catalog() {
    for mode in [
        "malformed",
        "wrong-id",
        "duplicate",
        "incomplete",
        "oversized",
        "stderr",
        "tail",
        "nonzero",
    ] {
        let fixture = Fixture::new(mode, 10_000);
        let isolation = fixture.launch();
        let receipt = wait_receipt(&isolation, &fixture.request.job);
        assert_eq!(receipt.observed_outcome, p::WorkerOutcome::Failed, "{mode}");
        assert!(
            isolation
                .result_path(&fixture.request.job)
                .unwrap()
                .is_none(),
            "{mode}"
        );
        seal(&isolation, &fixture.request.job);
    }
}

#[test]
#[ignore = "requires explicit compiled native MCP fixture; not installed acceptance"]
fn native_managed_mcp_revoke_disable_and_configuration_lock_cancellation() {
    for mode in ["revoke", "disable", "cancel-with-config-locked"] {
        let fixture = Fixture::new("hold", 10_000);
        let isolation = fixture.launch();
        let job = &fixture.request.job;
        let dir = worker_dir(&isolation, job);
        wait_file(&dir.join("workspace/requests.jsonl"));
        let until = Instant::now() + Duration::from_secs(5);
        while methods(&dir).is_empty() {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
        let pid: u32 = fs::read_to_string(dir.join("workspace/started.pid"))
            .unwrap()
            .parse()
            .unwrap();
        isolation.children.lock().unwrap()[&job.job_id]
            .native
            .as_ref()
            .unwrap()
            .verify_observed_process(pid)
            .unwrap();
        match mode {
            "revoke" => fixture.revoke(),
            "disable" => {
                let mut settings = fixture.settings.clone();
                settings.enabled = false;
                config_tests::stage_mcp_fixture(&mut fixture.config.lock().unwrap(), settings);
            }
            _ => {}
        }
        let locked = (mode == "cancel-with-config-locked").then(|| fixture.config.lock().unwrap());
        fs::write(dir.join("workspace/continue"), b"release").unwrap();
        if locked.is_some() {
            let started = Instant::now();
            let cancel = isolation.cancel_at(job, recovery_timestamp()).unwrap();
            assert_eq!(cancel.outcome, p::WorkerOutcome::Cancelled);
            assert!(started.elapsed() < Duration::from_secs(2));
        } else {
            assert_eq!(
                wait_receipt(&isolation, job).observed_outcome,
                p::WorkerOutcome::Failed
            );
        }
        assert_eq!(methods(&dir), ["initialize"], "no RPC after {mode}");
        drop(locked);
        seal(&isolation, job);
    }
}

#[test]
#[ignore = "requires explicit compiled native MCP fixture; not installed acceptance"]
fn native_managed_mcp_original_deadline_terminates_unresponsive_peer() {
    let fixture = Fixture::new("hang", 1_500);
    let started = Instant::now();
    let isolation = fixture.launch();
    let receipt = wait_receipt(&isolation, &fixture.request.job);
    assert_eq!(receipt.observed_outcome, p::WorkerOutcome::Failed);
    assert!(started.elapsed() < Duration::from_secs(4));
    let dir = worker_dir(&isolation, &fixture.request.job);
    assert_eq!(methods(&dir), ["initialize"]);
    seal(&isolation, &fixture.request.job);
}

mod call;
