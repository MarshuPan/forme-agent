use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use forme_capabilities::{
    AppApiRequest, Capability, CapabilityDescriptor, LocalPlugin, McpAllowlist, PluginContribution,
    PluginManifest, SkillBody, SkillDefinition, SkillMetadata, StdioMcpServer,
};
use forme_context::{ContextBudget, ContextHydrationRequest, ContextHydrator};
use forme_harness::{
    AppApiRuntimeConfig, CapabilityRuntimeConfig, CognitiveRuntimeConfig, DataProtectionConfig,
    McpRuntimeConfig, PluginRuntimeConfig, ProductionRuntimeBuilder, ProductionRuntimeConfig,
    RuntimeServiceStatus,
};
use forme_models::{ModelRuntimeConfig, ModelStrength};
use forme_protocol as p;
use forme_store::{
    ApprovalConsumptionClaim, ApprovalConsumptionLedger, BrainAuthorityStore, EventStore,
    SqliteEventStore, StoreOptions, APPROVAL_CONSUMPTION_SCHEMA_VERSION,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static NEXT_DATABASE: AtomicU64 = AtomicU64::new(1);

struct EnvironmentGuard {
    _lock: MutexGuard<'static, ()>,
    values: Vec<(&'static str, Option<String>)>,
}

impl EnvironmentGuard {
    fn model() -> Self {
        let lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let keys = [
            "FORME_MODEL_PROFILES_JSON",
            "FORME_MODEL_PROFILE_ID",
            "FORME_MODEL_PROVIDER",
            "FORME_MODEL_NAME",
            "FORME_MODEL_BASE_URL",
            "FORME_MODEL_API_KEY",
            "FORME_MODEL_CONTEXT_WINDOW",
            "FORME_MODEL_TIMEOUT_MS",
            "FORME_A6_MISSING_CREDENTIAL",
            "FORME_A6_PRESENT_CREDENTIAL",
        ];
        let values = keys
            .into_iter()
            .map(|key| {
                let previous = std::env::var(key).ok();
                match key {
                    "FORME_MODEL_PROFILES_JSON" => std::env::remove_var(key),
                    "FORME_MODEL_PROFILE_ID" => std::env::set_var(key, "model:test"),
                    "FORME_MODEL_PROVIDER" => std::env::set_var(key, "test-provider"),
                    "FORME_MODEL_NAME" => std::env::set_var(key, "test-model"),
                    "FORME_MODEL_BASE_URL" => std::env::set_var(key, "http://127.0.0.1:9/v1"),
                    "FORME_MODEL_API_KEY" => std::env::set_var(key, "local-test-secret"),
                    "FORME_MODEL_CONTEXT_WINDOW" => std::env::set_var(key, "32768"),
                    "FORME_MODEL_TIMEOUT_MS" => std::env::set_var(key, "1000"),
                    "FORME_A6_MISSING_CREDENTIAL" => std::env::remove_var(key),
                    "FORME_A6_PRESENT_CREDENTIAL" => {
                        std::env::set_var(key, "a6-private-test-value")
                    }
                    _ => unreachable!(),
                }
                (key, previous)
            })
            .collect();
        Self {
            _lock: lock,
            values,
        }
    }
}

impl EnvironmentGuard {
    /// Records the previous value before changing `key`; drop restores in
    /// reverse order, so repeated changes of one key unwind correctly.
    fn set(&mut self, key: &'static str, value: Option<&str>) {
        self.values.push((key, std::env::var(key).ok()));
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    /// Clears every host-provided runtime variable that could leak into a
    /// formal `from_environment` composition, then binds the test owner.
    fn formal(&mut self, owner: &str, path: &Path) {
        for key in FORMAL_RUNTIME_KEYS {
            self.set(key, None);
        }
        self.set("FORME_OWNER_ID", Some(owner));
        self.set("FORME_STORE_PATH", Some(path.to_str().unwrap()));
        self.set("FORME_WORKSPACE", Some("workspace:test"));
        self.set("FORME_AGENT_PROFILE", Some("agent:test"));
        self.set(
            "FORME_ARTIFACT_ROOT",
            Some(path.with_extension("artifacts").to_str().unwrap()),
        );
        self.set("FORME_NOTIFICATION_ENABLED", Some("false"));
    }
}

const FORMAL_RUNTIME_KEYS: [&str; 27] = [
    "FORME_OWNER_ID",
    "FORME_STORE_PATH",
    "FORME_WORKSPACE",
    "FORME_AGENT_PROFILE",
    "FORME_ARTIFACT_ROOT",
    "FORME_NOTIFICATION_ENABLED",
    "FORME_CAPABILITY_CONFIG",
    "FORME_CAPABILITY_BUDGET",
    "FORME_CAPABILITY_OUTPUT_BYTES",
    "FORME_CAPABILITY_TIMEOUT_MS",
    "FORME_FILE_ENABLED",
    "FORME_FILE_ROOTS",
    "FORME_SHELL_ENABLED",
    "FORME_SHELL_PROGRAMS",
    "FORME_SHELL_ROOTS",
    "FORME_SHELL_NETWORK",
    "FORME_BROWSER_ENABLED",
    "FORME_BROWSER_EXECUTABLE",
    "FORME_BROWSER_DRIVER",
    "FORME_BROWSER_ALLOWED_ORIGINS",
    "FORME_PTY_ENABLED",
    "FORME_PTY_PROGRAMS",
    "FORME_PTY_ROOTS",
    "FORME_COMPUTER_ENABLED",
    "FORME_COGNITIVE_TICK_ENABLED",
    "FORME_SCHEDULER_ENABLED",
    "FORME_BRAIN_LEASE_MS",
];

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        for (key, previous) in self.values.drain(..).rev() {
            match previous {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn database_path(label: &str) -> PathBuf {
    let id = NEXT_DATABASE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "forme-v1-core-brain-a-{}-{}-{}.db",
        label,
        std::process::id(),
        id
    ))
}

fn remove_database(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let candidate = PathBuf::from(format!("{}{suffix}", path.display()));
        let _ = std::fs::remove_file(candidate);
    }
}

fn config(owner: &str, path: &Path) -> ProductionRuntimeConfig {
    ProductionRuntimeConfig {
        schema_version: p::SchemaVersion(1),
        owner: p::OwnerPrincipalRef(owner.into()),
        store_path: path.to_path_buf(),
        workspace: p::WorkspaceRef("workspace:test".into()),
        agent_profile: p::AgentProfileRef("agent:test".into()),
        models: vec![ModelRuntimeConfig {
            id: "model:test".into(),
            provider: "test-provider".into(),
            model: "test-model".into(),
            base_url: "http://127.0.0.1:9/v1".into(),
            credential_env: "FORME_MODEL_API_KEY".into(),
            context_window: 32_768,
            tool_use: false,
            strength: ModelStrength::Standard,
            input_microunits_per_million: 0,
            output_microunits_per_million: 0,
            requests_per_minute: 60,
            tokens_per_minute: 1_000_000,
            timeout_ms: 1_000,
            fallback_group: Some("test-default".into()),
            default: true,
            tool_schema_versions: Vec::new(),
        }],
        capabilities: CapabilityRuntimeConfig::default(),
        scheduler_enabled: true,
        scheduler: p::SchedulerConfig {
            schema_version: p::SchemaVersion(1),
            tick: p::DurationMs(1_000),
            lease: p::DurationMs(30_000),
            max_claims_per_tick: 1,
        },
        cognition: CognitiveRuntimeConfig::default(),
        data: DataProtectionConfig {
            schema_version: p::SchemaVersion(1),
            artifact_root: path.with_extension("artifacts"),
        },
        authority_lease: p::DurationMs(30_000),
        context_limits: forme_context::ContextHydrationLimits::default(),
    }
}

fn context_request(label: &str, selected_skills: Vec<p::SkillRef>) -> ContextHydrationRequest {
    ContextHydrationRequest {
        schema_version: p::SchemaVersion(1),
        run: p::RunId(format!("run:{label}")),
        session: p::SessionId(format!("session:{label}")),
        workspace: p::WorkspaceRef("workspace:test".into()),
        scope: p::Scope("workspace:test".into()),
        brain_call: true,
        selected_skills,
        model: p::ModelProfileRef("model:test".into()),
        budget: ContextBudget {
            schema_version: p::SchemaVersion(1),
            max_tokens: 32_768,
            reserve: 1_024,
        },
    }
}

fn runtime_grant(
    capabilities: Vec<p::CapabilityRef>,
    permissions: Vec<p::PermissionRef>,
) -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:test".into()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities,
            permissions,
        },
        action_type: vec![
            p::ActionType::Observe,
            p::ActionType::Analyze,
            p::ActionType::Execute,
        ],
        risk_limit: p::Risk::Low,
        approval_rule: p::ApprovalRule::Allow,
        budget: p::Budget("units:100000".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: 0,
            expires_at: i64::MAX,
            max_turns: 32,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    }
}

fn with_budget(mut grant: p::AutonomyEnvelope, budget: &str) -> p::AutonomyEnvelope {
    grant.budget = p::Budget(budget.into());
    grant
}

fn remove_artifacts(path: &Path) {
    let _ = std::fs::remove_dir_all(path.with_extension("artifacts"));
}

fn app_api_config(
    id: &str,
    schema_digest: &str,
    credential_env: Option<&str>,
) -> AppApiRuntimeConfig {
    let capability = p::CapabilityRef(format!("capability:app-api:{id}"));
    let permission = p::PermissionRef(format!("permission:app-api:{id}"));
    AppApiRuntimeConfig {
        provider_id: p::ProviderId(id.into()),
        base_url: format!("http://127.0.0.1:9/{id}/"),
        schema_digest: p::SchemaDigest(schema_digest.into()),
        credential_env: credential_env.map(str::to_owned),
        scope: p::Scope("workspace:test".into()),
        capability: capability.clone(),
        permission: permission.clone(),
        allowed_mutations: vec![p::AppApiMutationMethod::Post],
        requests_per_minute: 10,
        timeout: p::DurationMs(1_000),
        max_response_bytes: 4_096,
        enabled: true,
        trust: Some(p::TrustTier::ApprovedSource),
        grant: Some(runtime_grant(vec![capability], vec![permission])),
    }
}

fn base_skill() -> SkillDefinition {
    SkillDefinition {
        schema_version: p::SchemaVersion(1),
        metadata: SkillMetadata {
            schema_version: p::SchemaVersion(1),
            id: p::SkillRef("skill:base".into()),
            summary: "base repository skill".into(),
            scope: p::Scope("workspace:test".into()),
            version: p::Version(1),
            trust: p::TrustTier::OwnerInput,
        },
        body: SkillBody("use the bounded base repository workflow".into()),
    }
}

fn skill_plugin() -> LocalPlugin {
    LocalPlugin {
        schema_version: p::SchemaVersion(1),
        manifest: PluginManifest {
            schema_version: p::SchemaVersion(1),
            id: p::PluginRef("plugin:a6-catalog".into()),
            manifest_ref: p::PluginManifestRef("manifest:a6-catalog".into()),
            trust: p::TrustTier::Untrusted,
            contributions: vec![
                PluginContribution::Skill(SkillDefinition {
                    schema_version: p::SchemaVersion(1),
                    metadata: SkillMetadata {
                        schema_version: p::SchemaVersion(1),
                        id: p::SkillRef("skill:plugin-catalog".into()),
                        summary: "plugin catalog workflow".into(),
                        scope: p::Scope("workspace:test".into()),
                        version: p::Version(1),
                        trust: p::TrustTier::Untrusted,
                    },
                    body: SkillBody("use the enabled plugin workflow".into()),
                }),
                PluginContribution::Tool(CapabilityDescriptor {
                    schema_version: p::SchemaVersion(1),
                    id: p::CapabilityRef("capability:plugin-unbound-tool".into()),
                    capability: Capability::Tool(p::ToolRef("tool:plugin-unbound".into())),
                    scope: p::Scope("workspace:test".into()),
                    permissions: vec![p::PermissionRef("permission:plugin-unbound".into())],
                    risk: Some(p::Risk::Low),
                    enabled: true,
                }),
            ],
        },
    }
}

fn mcp_server() -> StdioMcpServer {
    let executable = std::env::current_exe().expect("test executable");
    StdioMcpServer {
        schema_version: p::SchemaVersion(1),
        provider_id: p::ProviderId("mcp:a6".into()),
        command: executable.to_string_lossy().into_owned(),
        args: vec![
            "--exact".into(),
            "a6_mcp_fixture_child".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
        ],
        env: vec![("FORME_A6_MCP_CHILD".into(), "1".into())],
        allowlist: McpAllowlist {
            schema_version: p::SchemaVersion(1),
            tools: vec!["read_allowed".into()],
            resources: vec!["forme://allowed".into()],
        },
        timeout: p::DurationMs(2_000),
        declared_capabilities: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![p::CapabilityRef("mcp:mcp:a6:read_allowed".into())],
            permissions: vec![p::PermissionRef("permission:mcp-read".into())],
        },
    }
}

#[test]
fn production_builder_composes_one_non_degraded_brain_and_formal_context() {
    let _env = EnvironmentGuard::model();
    let path = database_path("composition");
    let runtime = ProductionRuntimeBuilder::from_config(config("owner:test", &path))
        .expect("valid production config")
        .build()
        .expect("production runtime should compose");

    assert!(runtime.harness().has_production_composition());
    assert_eq!(runtime.manifest().workspace.0, "workspace:test");
    assert_eq!(runtime.manifest().agent_profile.0, "agent:test");
    assert!(runtime.manifest().validate().is_ok());
    assert!(runtime
        .services()
        .doctor
        .checks
        .iter()
        .all(|check| check.status != RuntimeServiceStatus::Blocking));
    assert!(runtime
        .services()
        .doctor
        .checks
        .iter()
        .any(|check| check.name == "mcp-registry"
            && check.status == RuntimeServiceStatus::Unsupported));
    // Every service the composition root owns reports an explicit state.
    for name in [
        "authority-store",
        "owner-brain-aggregates",
        "model-registry",
        "model:model:test",
        "event-sourced-memory",
        "cognitive-runtime",
        "coordination-reasoner",
        "orchestration-library",
        "proactivity-engine",
        "scheduler",
        "cognitive-tick",
        "context-hydrator",
        "execution-backend-registry",
        "mcp-registry",
        "plugin-registry",
        "app-api-registry",
        "evaluation-verifier",
        "federation-runtime",
        "ecosystem-runtime",
    ] {
        assert!(
            runtime
                .doctor()
                .checks
                .iter()
                .any(|check| check.name == name),
            "the doctor has no explicit state for {name}"
        );
    }

    let request = ContextHydrationRequest {
        schema_version: p::SchemaVersion(1),
        run: p::RunId("run:context-test".into()),
        session: p::SessionId("session:context-test".into()),
        workspace: p::WorkspaceRef("workspace:test".into()),
        scope: p::Scope("workspace:test".into()),
        brain_call: true,
        selected_skills: Vec::new(),
        model: runtime.manifest().model_profiles[0].clone(),
        budget: ContextBudget {
            schema_version: p::SchemaVersion(1),
            max_tokens: 32_768,
            reserve: 1_024,
        },
    };
    let hydrated = runtime
        .services()
        .context_hydrator
        .hydrate(&request)
        .expect("formal context must have usable sources");
    assert!(!hydrated.sources.rules.is_empty());
    assert!(hydrated.sources.agent_workspace.is_some());

    let mut out_of_scope = request.clone();
    out_of_scope.workspace = p::WorkspaceRef("workspace:other".into());
    out_of_scope.scope = p::Scope("workspace:other".into());
    assert!(runtime
        .services()
        .context_hydrator
        .hydrate(&out_of_scope)
        .is_err());

    drop(runtime);
    remove_database(&path);
}

#[test]
fn production_brain_authority_and_manifest_survive_restart_without_synthetic_facts() {
    let _env = EnvironmentGuard::model();
    let path = database_path("restart");
    let first = ProductionRuntimeBuilder::from_config(config("owner:restart", &path))
        .unwrap()
        .build()
        .unwrap();
    let first_manifest = first.manifest().clone();
    let first_refs = first_manifest.aggregate_refs.clone();
    let first_events = first
        .services()
        .store
        .read_run(first_refs.memory_cognition.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    drop(first);

    let second = ProductionRuntimeBuilder::from_config(config("owner:restart", &path))
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(second.manifest().aggregate_refs, first_refs);
    assert_eq!(second.manifest().digest, first_manifest.digest);
    let second_events = second
        .services()
        .store
        .read_run(first_refs.memory_cognition.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(second_events.len(), first_events.len());
    assert!(
        second_events
            .iter()
            .map(|event| event.stream_seq)
            .collect::<BTreeSet<_>>()
            .len()
            == second_events.len()
    );

    drop(second);
    let inspection = SqliteEventStore::open(&path, StoreOptions::default()).unwrap();
    let metadata = inspection.brain_authority().unwrap().unwrap();
    assert_eq!(metadata.aggregates, first_refs.as_map());
    assert!(inspection
        .run_ids()
        .unwrap()
        .contains(&first_refs.memory_cognition));
    drop(inspection);
    remove_database(&path);
}

#[test]
fn production_runtime_approval_consumption_remains_closed_after_restart() {
    let _env = EnvironmentGuard::model();
    let path = database_path("approval-restart");
    let owner = p::VerifiedPrincipal("owner:approval-restart".into());
    let plan_digest = p::sha256_content_digest(b"production-approval-plan");
    let action_digest = p::sha256_content_digest(b"production-approval-action");
    let claim = ApprovalConsumptionClaim {
        schema_version: APPROVAL_CONSUMPTION_SCHEMA_VERSION,
        approval_id: p::ApprovalId("approval:production-restart".into()),
        approver: owner.clone(),
        nonce: p::Nonce("nonce:production-restart".into()),
        plan_digest: p::PlanDigest(plan_digest.0.clone()),
        normalized_action_digest: action_digest,
        affected_resources: vec![p::ResourceRef("workspace:production-restart".into())],
        policy_version: p::Version(1),
        tool_schema_version: p::Version(1),
        now: 1,
    };

    let first = ProductionRuntimeBuilder::from_config(config(&owner.0, &path))
        .unwrap()
        .build()
        .unwrap();
    assert!(first
        .services()
        .store
        .claim_approval(&claim)
        .expect("production runtime must expose its durable approval ledger"));
    drop(first);

    let second = ProductionRuntimeBuilder::from_config(config(&owner.0, &path))
        .unwrap()
        .build()
        .unwrap();
    assert!(!second
        .services()
        .store
        .claim_approval(&claim)
        .expect("replayed approval nonce must be rejected as an equivalent retry"));
    drop(second);
    remove_database(&path);
}

#[test]
fn production_builder_fences_wrong_owner_and_second_live_writer() {
    let _env = EnvironmentGuard::model();
    let path = database_path("authority");
    let first = ProductionRuntimeBuilder::from_config(config("owner:a", &path))
        .unwrap()
        .build()
        .unwrap();

    let second_error = match ProductionRuntimeBuilder::from_config(config("owner:a", &path))
        .unwrap()
        .build()
    {
        Ok(_) => panic!("a second live production writer must be rejected"),
        Err(error) => error,
    };
    assert!(second_error.0.contains("held by another runtime"));

    drop(first);
    let wrong_owner_error = match ProductionRuntimeBuilder::from_config(config("owner:b", &path))
        .unwrap()
        .build()
    {
        Ok(_) => panic!("a different owner must not inherit the brain"),
        Err(error) => error,
    };
    assert!(wrong_owner_error.0.contains("does not match"));

    remove_database(&path);
}

#[test]
fn idle_runtime_heartbeat_keeps_the_sole_writer_lease_and_drop_releases_it() {
    let _env = EnvironmentGuard::model();
    let path = database_path("heartbeat");
    let mut short_lease = config("owner:heartbeat", &path);
    short_lease.authority_lease = p::DurationMs(300);
    let first = ProductionRuntimeBuilder::from_config(short_lease.clone())
        .unwrap()
        .build()
        .unwrap();
    // Idle well past the TTL: no run, no write. Only the heartbeat renews.
    std::thread::sleep(Duration::from_millis(1_000));
    let rejected = ProductionRuntimeBuilder::from_config(short_lease.clone())
        .unwrap()
        .build();
    assert!(
        matches!(&rejected, Err(error) if error.0.contains("held by another runtime")),
        "an idle live runtime must keep its lease"
    );
    drop(rejected);
    drop(first);
    let second = ProductionRuntimeBuilder::from_config(short_lease)
        .unwrap()
        .build()
        .expect("dropping the runtime releases the lease immediately");
    drop(second);
    remove_database(&path);
}

#[test]
fn optional_backends_degrade_without_entering_the_visible_toolset() {
    let _env = EnvironmentGuard::model();
    let path = database_path("optional-backends");
    let mut runtime_config = config("owner:optional-backends", &path);
    runtime_config.capabilities.notification_enabled = false;
    runtime_config.capabilities.file.enabled = true;
    runtime_config.capabilities.shell.enabled = true;
    runtime_config.capabilities.browser.enabled = true;
    runtime_config.capabilities.browser.executable = None;
    runtime_config.capabilities.browser.allowed_origins.clear();
    runtime_config.capabilities.computer.enabled = true;
    runtime_config.capabilities.computer.surfaces.clear();
    runtime_config.capabilities.pty.enabled = true;

    let runtime = ProductionRuntimeBuilder::from_config(runtime_config)
        .expect("missing optional backend configuration is not a core config failure")
        .build()
        .expect("the owner brain must build with unavailable optional backends");
    for name in [
        "file-backend",
        "shell-backend",
        "browser-backend",
        "computer-backend",
        "pty-backend",
    ] {
        assert!(runtime.doctor().checks.iter().any(|check| {
            check.name == name && check.status == RuntimeServiceStatus::Unsupported
        }));
    }
    for backend in [
        p::BackendKind::File,
        p::BackendKind::Shell,
        p::BackendKind::Browser,
        p::BackendKind::Computer,
        p::BackendKind::Pty,
    ] {
        assert!(runtime.services().backends.backend(backend).is_err());
    }
    let hydrated = runtime
        .services()
        .context_hydrator
        .hydrate(&context_request("optional-backends", Vec::new()))
        .unwrap();
    assert!(hydrated.sources.tool_schema.entries.is_empty());

    drop(runtime);
    remove_database(&path);
    remove_artifacts(&path);
}

#[test]
fn app_api_missing_credential_or_schema_is_not_exposed() {
    let _env = EnvironmentGuard::model();
    let path = database_path("app-api-unavailable");
    let mut runtime_config = config("owner:app-api-unavailable", &path);
    runtime_config.capabilities.notification_enabled = false;
    runtime_config.capabilities.app_api = vec![
        app_api_config(
            "app-api:missing-credential",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Some("FORME_A6_MISSING_CREDENTIAL"),
        ),
        app_api_config("app-api:missing-schema", "", None),
    ];

    let runtime = ProductionRuntimeBuilder::from_config(runtime_config)
        .expect("an unavailable optional App API is not a core config failure")
        .build()
        .expect("the owner brain must build without half-exposing App APIs");
    assert!(runtime
        .services()
        .backends
        .backend(p::BackendKind::AppApi)
        .is_err());
    for id in ["app-api:missing-credential", "app-api:missing-schema"] {
        assert!(runtime.doctor().checks.iter().any(|check| {
            check.name == format!("app-api:{id}")
                && check.status == RuntimeServiceStatus::Unsupported
        }));
    }
    let hydrated = runtime
        .services()
        .context_hydrator
        .hydrate(&context_request("app-api-unavailable", Vec::new()))
        .unwrap();
    assert!(hydrated.sources.tool_schema.entries.is_empty());

    drop(runtime);
    remove_database(&path);
}

#[test]
fn plugin_skill_is_dynamic_but_unbound_plugin_tool_is_never_executable() {
    let _env = EnvironmentGuard::model();
    let path = database_path("plugin-catalog");
    let mut runtime_config = config("owner:plugin-catalog", &path);
    runtime_config.capabilities.notification_enabled = false;
    runtime_config.capabilities.skills = vec![base_skill()];
    let plugin = skill_plugin();
    let plugin_id = plugin.id();
    runtime_config.capabilities.plugins = vec![PluginRuntimeConfig {
        plugin,
        enabled: true,
        trust: Some(p::TrustTier::ApprovedSource),
        grant: Some(runtime_grant(
            vec![
                p::CapabilityRef("skill:plugin-catalog".into()),
                p::CapabilityRef("capability:plugin-unbound-tool".into()),
            ],
            vec![p::PermissionRef("permission:plugin-unbound".into())],
        )),
        activate_on_start: true,
    }];

    let runtime = ProductionRuntimeBuilder::from_config(runtime_config)
        .unwrap()
        .build()
        .expect("the trusted and granted plugin should activate");
    let selected = runtime
        .services()
        .context_hydrator
        .hydrate(&context_request(
            "plugin-enabled",
            vec![p::SkillRef("skill:plugin-catalog".into())],
        ))
        .unwrap();
    assert!(selected
        .sources
        .skills_metadata
        .iter()
        .any(|skill| skill.id == p::SkillRef("skill:base".into())));
    assert!(selected
        .sources
        .skills_metadata
        .iter()
        .any(|skill| skill.id == p::SkillRef("skill:plugin-catalog".into())));
    assert!(selected
        .sources
        .loaded_skill_bodies
        .iter()
        .any(|skill| skill.id == p::SkillRef("skill:plugin-catalog".into())));
    assert!(!selected
        .sources
        .tool_schema
        .entries
        .iter()
        .any(|entry| entry.tool == p::ToolRef("tool:plugin-unbound".into())));

    runtime.services().plugins.disable(plugin_id).unwrap();
    let disabled = runtime
        .services()
        .context_hydrator
        .hydrate(&context_request("plugin-disabled", Vec::new()))
        .unwrap();
    assert!(disabled
        .sources
        .skills_metadata
        .iter()
        .all(|skill| skill.id != p::SkillRef("skill:plugin-catalog".into())));
    assert!(runtime
        .services()
        .backends
        .backend(p::BackendKind::Shell)
        .is_err());

    drop(runtime);
    remove_database(&path);
}

#[test]
fn production_mcp_catalog_exposes_only_allowlisted_discovery() {
    let _env = EnvironmentGuard::model();
    let path = database_path("mcp-allowlist");
    let mut runtime_config = config("owner:mcp-allowlist", &path);
    runtime_config.capabilities.notification_enabled = false;
    let server = mcp_server();
    let server_id = server.provider_id.clone();
    runtime_config.capabilities.mcp = vec![McpRuntimeConfig {
        grant: Some(runtime_grant(
            server.declared_capabilities.capabilities.clone(),
            server.declared_capabilities.permissions.clone(),
        )),
        server,
        inherit_env: Vec::new(),
        enabled: true,
        trust: Some(p::TrustTier::ApprovedSource),
        discover_on_start: true,
    }];

    let runtime = ProductionRuntimeBuilder::from_config(runtime_config)
        .unwrap()
        .build()
        .expect("the MCP fixture should discover at startup");
    let discovery = runtime
        .services()
        .mcp
        .discovery(&server_id)
        .unwrap_or_else(|| {
            panic!(
                "MCP discovery snapshot; doctor={:?}; failures={:?}",
                runtime.doctor(),
                runtime.services().mcp.take_failures()
            )
        });
    assert_eq!(
        discovery
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["read_allowed"]
    );
    assert_eq!(
        discovery
            .resources
            .iter()
            .map(|resource| resource.uri.as_str())
            .collect::<Vec<_>>(),
        vec!["forme://allowed"]
    );
    let hydrated = runtime
        .services()
        .context_hydrator
        .hydrate(&context_request("mcp-allowlist", Vec::new()))
        .unwrap();
    assert_eq!(hydrated.sources.tool_schema.entries.len(), 1);
    assert!(hydrated.sources.tool_schema.entries[0]
        .tool
        .0
        .ends_with("read_allowed"));
    assert!(hydrated
        .sources
        .tool_schema
        .entries
        .iter()
        .all(|entry| !entry.tool.0.contains("hidden_mutation")));
    assert!(runtime
        .services()
        .backends
        .backend(p::BackendKind::Mcp)
        .is_ok());

    drop(runtime);
    remove_database(&path);
}

#[test]
fn multiple_app_api_connectors_route_by_plan_and_public_artifacts_hide_secrets() {
    let _env = EnvironmentGuard::model();
    let path = database_path("app-api-routing");
    let mut runtime_config = config("owner:app-api-routing", &path);
    runtime_config.capabilities.notification_enabled = false;
    runtime_config.capabilities.app_api = vec![
        app_api_config(
            "app-api:alpha",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Some("FORME_A6_PRESENT_CREDENTIAL"),
        ),
        app_api_config(
            "app-api:beta",
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            Some("FORME_A6_PRESENT_CREDENTIAL"),
        ),
    ];
    let runtime = ProductionRuntimeBuilder::from_config(runtime_config)
        .unwrap()
        .build()
        .expect("both App API connectors should be active");
    let backend = runtime
        .services()
        .backends
        .backend(p::BackendKind::AppApi)
        .expect("composite App API backend");
    for id in ["app-api:alpha", "app-api:beta"] {
        let intent = runtime
            .services()
            .app_api
            .prepare(
                p::ProviderId(id.into()),
                AppApiRequest {
                    schema_version: p::SchemaVersion(1),
                    resource: "records/current".into(),
                    operation: p::AppApiOperation::Read,
                    source: p::Source::UserTurn,
                    goal: p::GoalRef("read a scoped connector resource".into()),
                    action_type: p::ActionType::Observe,
                    risk: p::Risk::Low,
                    participant: None,
                    representation: None,
                    disclosure_request: None,
                    requested_at: 1,
                    estimated_output_bytes: 1_024,
                },
            )
            .unwrap();
        let plan = backend.plan(&intent).expect("connector-aware routing");
        let p::ActionParameters::AppApi(spec) = plan.intent.parameters else {
            panic!("App API plan must preserve connector parameters");
        };
        assert_eq!(spec.connector, p::ProviderId(id.into()));
    }

    let manifest = format!("{:?}", runtime.manifest());
    let doctor = format!("{:?}", runtime.doctor());
    for public_artifact in [&manifest, &doctor] {
        assert!(!public_artifact.contains("a6-private-test-value"));
        assert!(!public_artifact.contains("FORME_A6_PRESENT_CREDENTIAL"));
        assert!(!public_artifact.contains(path.to_string_lossy().as_ref()));
    }

    drop(runtime);
    remove_database(&path);
}

#[test]
fn provider_budget_mismatch_is_explicit_and_never_enters_the_toolset() {
    let _env = EnvironmentGuard::model();
    let path = database_path("provider-budget-mismatch");
    let mut runtime_config = config("owner:provider-budget-mismatch", &path);
    runtime_config.capabilities.notification_enabled = false;
    let compatible = app_api_config(
        "app-api:compatible-budget",
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        Some("FORME_A6_PRESENT_CREDENTIAL"),
    );
    let mut incompatible = app_api_config(
        "app-api:incompatible-budget",
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        Some("FORME_A6_PRESENT_CREDENTIAL"),
    );
    incompatible.grant = incompatible
        .grant
        .take()
        .map(|grant| with_budget(grant, "units:other"));
    runtime_config.capabilities.app_api = vec![compatible, incompatible];

    let runtime = ProductionRuntimeBuilder::from_config(runtime_config)
        .unwrap()
        .build()
        .expect("one incompatible optional provider must not split runtime state");
    assert!(runtime.doctor().checks.iter().any(|check| {
        check.name == "app-api:app-api:compatible-budget"
            && check.status == RuntimeServiceStatus::Ready
    }));
    assert!(runtime.doctor().checks.iter().any(|check| {
        check.name == "app-api:app-api:incompatible-budget"
            && check.status == RuntimeServiceStatus::Unsupported
    }));

    let hydrated = runtime
        .services()
        .context_hydrator
        .hydrate(&context_request("provider-budget-mismatch", Vec::new()))
        .unwrap();
    assert_eq!(hydrated.sources.tool_schema.entries.len(), 1);
    assert!(hydrated.sources.tool_schema.entries[0]
        .schema
        .contains("compatible-budget"));
    assert!(runtime.manifest().service_states.iter().any(|state| {
        state.name == "app-api:app-api:incompatible-budget"
            && state.status == RuntimeServiceStatus::Unsupported
    }));
    assert!(runtime
        .services()
        .app_api
        .prepare(
            p::ProviderId("app-api:incompatible-budget".into()),
            AppApiRequest {
                schema_version: p::SchemaVersion(1),
                resource: "records/current".into(),
                operation: p::AppApiOperation::Read,
                source: p::Source::UserTurn,
                goal: p::GoalRef("prove incompatible provider exclusion".into()),
                action_type: p::ActionType::Observe,
                risk: p::Risk::Low,
                participant: None,
                representation: None,
                disclosure_request: None,
                requested_at: 1,
                estimated_output_bytes: 1_024,
            },
        )
        .is_err());

    drop(runtime);
    remove_database(&path);
    remove_artifacts(&path);
}

fn formal_capability_document(inherited_name: &str) -> serde_json::Value {
    let server = mcp_server();
    let mcp_capability = p::CapabilityRef("mcp:mcp:formal:read_allowed".into());
    let mcp_permission = p::PermissionRef("permission:mcp-read".into());
    let app_capability = p::CapabilityRef("capability:app-api:formal".into());
    let app_permission = p::PermissionRef("permission:app-api:formal".into());
    serde_json::json!({
        "schema_version": 1,
        "skills": [{
            "id": "skill:formal-base",
            "summary": "formal repository workflow",
            "scope": "workspace:test",
            "trust": "OwnerInput",
            "body": "use the formal repository workflow"
        }],
        "mcp": [{
            "provider_id": "mcp:formal",
            "command": server.command,
            "args": server.args,
            "env": [
                { "name": "FORME_A6_MCP_CHILD", "value": "1" },
                { "name": "FORME_A6_REQUIRE_INHERITED", "value": "1" }
            ],
            "inherit_env": [inherited_name],
            "allowlist": { "tools": ["read_allowed"], "resources": ["forme://allowed"] },
            "timeout": 2000,
            "declared_capabilities": {
                "schema_version": 1,
                "capabilities": [mcp_capability.0.clone()],
                "permissions": [mcp_permission.0.clone()]
            },
            "enabled": true,
            "trust": "ApprovedSource",
            "grant": serde_json::to_value(runtime_grant(
                vec![mcp_capability],
                vec![mcp_permission],
            )).unwrap(),
            "discover_on_start": true
        }],
        "plugins": [{
            "id": "plugin:formal-catalog",
            "manifest_ref": "manifest:formal-catalog",
            "skills": [{
                "id": "skill:formal-plugin",
                "summary": "formal plugin workflow",
                "scope": "workspace:test",
                "body": "use the formal plugin workflow"
            }],
            "enabled": true,
            "trust": "ApprovedSource",
            "grant": serde_json::to_value(runtime_grant(
                vec![p::CapabilityRef("skill:formal-plugin".into())],
                Vec::new(),
            )).unwrap(),
            "activate_on_start": true
        }],
        "app_api": [{
            "provider_id": "app-api:formal",
            "base_url": "http://127.0.0.1:9/formal/",
            "schema_digest": "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            "credential_env": "FORME_A6_PRESENT_CREDENTIAL",
            "scope": "workspace:test",
            "capability": app_capability.0.clone(),
            "permission": app_permission.0.clone(),
            "allowed_mutations": ["Post"],
            "requests_per_minute": 10,
            "timeout": 1000,
            "max_response_bytes": 4096,
            "enabled": true,
            "trust": "ApprovedSource",
            "grant": serde_json::to_value(runtime_grant(
                vec![app_capability],
                vec![app_permission],
            )).unwrap()
        }]
    })
}

fn write_document(path: &Path, document: &serde_json::Value) -> PathBuf {
    let document_path = path.with_extension("capabilities.json");
    std::fs::write(&document_path, serde_json::to_vec_pretty(document).unwrap()).unwrap();
    document_path
}

fn doctor_status(
    runtime: &forme_harness::ProductionRuntime,
    name: &str,
) -> Option<RuntimeServiceStatus> {
    runtime
        .doctor()
        .checks
        .iter()
        .find(|check| check.name == name)
        .map(|check| check.status)
}

#[test]
fn formal_environment_registers_owner_configured_providers_through_one_lifecycle() {
    let mut env = EnvironmentGuard::model();
    let path = database_path("formal-capabilities");
    env.formal("owner:formal-capabilities", &path);
    env.set("FORME_A6_INHERITED", Some("inherited-marker"));
    let document_path = write_document(&path, &formal_capability_document("FORME_A6_INHERITED"));
    env.set(
        "FORME_CAPABILITY_CONFIG",
        Some(document_path.to_str().unwrap()),
    );

    let runtime = ProductionRuntimeBuilder::from_environment()
        .expect("the formal environment must accept the owner capability document")
        .build()
        .expect("the formal environment must compose the configured providers");
    for name in [
        "mcp:mcp:formal",
        "mcp-registry",
        "plugin:plugin:formal-catalog",
        "plugin-registry",
        "app-api:app-api:formal",
        "app-api-registry",
    ] {
        assert_eq!(
            doctor_status(&runtime, name),
            Some(RuntimeServiceStatus::Ready),
            "{name} should be ready; doctor={:?}",
            runtime.doctor()
        );
    }

    let hydrated = runtime
        .services()
        .context_hydrator
        .hydrate(&context_request(
            "formal-capabilities",
            vec![p::SkillRef("skill:formal-base".into())],
        ))
        .unwrap();
    let skills = hydrated
        .sources
        .skills_metadata
        .iter()
        .map(|skill| skill.id.0.as_str())
        .collect::<BTreeSet<_>>();
    assert!(skills.contains("skill:formal-base") && skills.contains("skill:formal-plugin"));
    assert_eq!(
        hydrated
            .sources
            .loaded_skill_bodies
            .iter()
            .map(|body| body.id.0.as_str())
            .collect::<Vec<_>>(),
        vec!["skill:formal-base"],
        "only the selected skill body may be loaded"
    );
    let tools = hydrated
        .sources
        .tool_schema
        .entries
        .iter()
        .map(|entry| entry.tool.0.clone())
        .collect::<Vec<_>>();
    assert_eq!(tools.len(), 2, "tools={tools:?}");
    assert!(tools.iter().any(|tool| tool.ends_with("read_allowed")));
    assert!(tools.iter().any(|tool| tool == "capability:app-api:formal"));

    let manifest = format!("{:?}", runtime.manifest());
    let doctor = format!("{:?}", runtime.doctor());
    let mcp_config = format!("{:?}", runtime.config().capabilities.mcp);
    for public in [&manifest, &doctor, &mcp_config] {
        assert!(!public.contains("a6-private-test-value"));
        assert!(!public.contains("inherited-marker"));
        assert!(!public.contains(document_path.to_string_lossy().as_ref()));
    }
    for public in [&manifest, &doctor] {
        assert!(!public.contains(path.to_string_lossy().as_ref()));
        assert!(!public.contains(&mcp_server().command));
    }

    drop(runtime);
    remove_database(&path);
    remove_artifacts(&path);
    let _ = std::fs::remove_file(document_path);
}

#[test]
fn formal_capability_document_fails_closed_without_echoing_private_paths() {
    let mut env = EnvironmentGuard::model();
    let path = database_path("formal-document-invalid");
    env.formal("owner:formal-document-invalid", &path);

    let mut unknown_field = formal_capability_document("FORME_A6_INHERITED");
    unknown_field["mcp"][0]["auto_trust"] = serde_json::json!(true);
    let mut future_version = formal_capability_document("FORME_A6_INHERITED");
    future_version["schema_version"] = serde_json::json!(2);
    let mut self_declared_trust = formal_capability_document("FORME_A6_INHERITED");
    self_declared_trust["plugins"][0]["manifest_trust"] = serde_json::json!("OwnerInput");
    for (label, document) in [
        ("unknown", unknown_field),
        ("future", future_version),
        ("self-trust", self_declared_trust),
    ] {
        let document_path = write_document(&path, &document);
        env.set(
            "FORME_CAPABILITY_CONFIG",
            Some(document_path.to_str().unwrap()),
        );
        let error = match ProductionRuntimeBuilder::from_environment() {
            Ok(_) => panic!("{label} document must fail closed"),
            Err(error) => error,
        };
        assert!(!error.0.contains(document_path.to_string_lossy().as_ref()));
        let _ = std::fs::remove_file(document_path);
    }

    let missing = path.with_extension("missing.json");
    env.set("FORME_CAPABILITY_CONFIG", Some(missing.to_str().unwrap()));
    let error = match ProductionRuntimeBuilder::from_environment() {
        Ok(_) => panic!("an unreadable capability document must fail closed"),
        Err(error) => error,
    };
    assert!(!error.0.contains(missing.to_string_lossy().as_ref()));
    assert!(
        !path.exists(),
        "a rejected configuration must not create the store"
    );
}

#[test]
fn providers_without_owner_binding_or_environment_stay_invisible() {
    let mut env = EnvironmentGuard::model();
    let path = database_path("formal-unbound");
    env.formal("owner:formal-unbound", &path);
    let mut document = formal_capability_document("FORME_A6_UNSET_INHERITED");
    let mut untrusted_mcp = document["mcp"][0].clone();
    untrusted_mcp["provider_id"] = serde_json::json!("mcp:untrusted");
    untrusted_mcp["inherit_env"] = serde_json::json!([]);
    untrusted_mcp["trust"] = serde_json::Value::Null;
    document["mcp"].as_array_mut().unwrap().push(untrusted_mcp);
    document["plugins"][0]["grant"] = serde_json::Value::Null;
    document["app_api"][0]["trust"] = serde_json::Value::Null;
    let document_path = write_document(&path, &document);
    env.set(
        "FORME_CAPABILITY_CONFIG",
        Some(document_path.to_str().unwrap()),
    );

    let runtime = ProductionRuntimeBuilder::from_environment()
        .unwrap()
        .build()
        .expect("unbound optional providers must not block the owner brain");
    assert_eq!(
        doctor_status(&runtime, "mcp:mcp:formal"),
        Some(RuntimeServiceStatus::Unsupported),
        "a missing inherited variable is an unavailable environment"
    );
    for name in [
        "mcp:mcp:untrusted",
        "plugin:plugin:formal-catalog",
        "app-api:app-api:formal",
    ] {
        assert_eq!(
            doctor_status(&runtime, name),
            Some(RuntimeServiceStatus::Disabled),
            "{name} must wait for an explicit owner trust and grant binding"
        );
    }
    assert!(runtime
        .services()
        .mcp
        .discovery(&p::ProviderId("mcp:untrusted".into()))
        .is_none());
    let hydrated = runtime
        .services()
        .context_hydrator
        .hydrate(&context_request("formal-unbound", Vec::new()))
        .unwrap();
    assert!(hydrated.sources.tool_schema.entries.is_empty());
    assert!(hydrated
        .sources
        .skills_metadata
        .iter()
        .all(|skill| skill.id.0 != "skill:formal-plugin"));
    for backend in [p::BackendKind::Mcp, p::BackendKind::AppApi] {
        assert!(runtime.services().backends.backend(backend).is_err());
    }

    drop(runtime);
    remove_database(&path);
    remove_artifacts(&path);
    let _ = std::fs::remove_file(document_path);
}

#[test]
fn a6_mcp_fixture_child() {
    if std::env::var("FORME_A6_MCP_CHILD").as_deref() != Ok("1") {
        return;
    }
    // A formal-config server declares an inherited variable. Answering only
    // when it arrived proves the composition root resolved and forwarded it
    // into the otherwise cleared child environment.
    if std::env::var("FORME_A6_REQUIRE_INHERITED").as_deref() == Ok("1")
        && std::env::var("FORME_A6_INHERITED").as_deref() != Ok("inherited-marker")
    {
        return;
    }
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    // libtest prints the test label without a trailing newline. Separate the
    // first JSON-RPC response so the parent can parse it as a complete line.
    writeln!(stdout).unwrap();
    stdout.flush().unwrap();
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        let Ok(request) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let Some(method) = request.get("method").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if method == "notifications/initialized" {
            continue;
        }
        let id = request.get("id").cloned();
        let result = match method {
            "initialize" => serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "serverInfo": { "name": "forme-a6-fixture", "version": "1" }
            }),
            "tools/list" => serde_json::json!({
                "tools": [
                    {
                        "name": "read_allowed",
                        "description": "read a bounded fixture",
                        "inputSchema": { "type": "object" }
                    },
                    {
                        "name": "hidden_mutation",
                        "inputSchema": { "type": "object" }
                    }
                ]
            }),
            "resources/list" => serde_json::json!({
                "resources": [
                    { "uri": "forme://allowed", "name": "allowed" },
                    { "uri": "forme://hidden", "name": "hidden" }
                ]
            }),
            _ => continue,
        };
        writeln!(
            stdout,
            "{}",
            serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })
        )
        .unwrap();
        stdout.flush().unwrap();
        if method == "resources/list" {
            std::thread::sleep(Duration::from_millis(10));
            return;
        }
    }
}
