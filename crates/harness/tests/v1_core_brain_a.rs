use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use forme_context::{ContextBudget, ContextHydrationRequest, ContextHydrator};
use forme_harness::{ProductionRuntimeBuilder, ProductionRuntimeConfig, RuntimeServiceStatus};
use forme_models::{ModelRegistryConfig, ModelRuntimeConfig, ModelStrength};
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
        let lock = ENV_LOCK.lock().expect("environment lock");
        let keys = [
            "FORME_MODEL_PROFILES_JSON",
            "FORME_MODEL_PROFILE_ID",
            "FORME_MODEL_PROVIDER",
            "FORME_MODEL_NAME",
            "FORME_MODEL_BASE_URL",
            "FORME_MODEL_API_KEY",
            "FORME_MODEL_CONTEXT_WINDOW",
            "FORME_MODEL_TIMEOUT_MS",
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
        models: ModelRegistryConfig {
            schema_version: p::SchemaVersion(1),
            profiles: vec![ModelRuntimeConfig {
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
        },
        scheduler_enabled: true,
        scheduler: p::SchedulerConfig {
            schema_version: p::SchemaVersion(1),
            tick: p::DurationMs(1_000),
            lease: p::DurationMs(30_000),
            max_claims_per_tick: 1,
        },
        notification_enabled: true,
        authority_lease: p::DurationMs(30_000),
        context_limits: forme_context::ContextHydrationLimits::default(),
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
