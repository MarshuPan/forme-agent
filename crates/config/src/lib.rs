//! Typed layered configuration, secret indirection, and ConfigDoctor (prd/16).
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use forme_protocol as p;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConfigLayerSource {
    Defaults,
    File,
    Environment,
    Cli,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfigKey(pub String);

impl ConfigKey {
    pub fn new(value: impl Into<String>) -> p::Result<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(p::Error("configuration key cannot be empty".into()));
        }
        Ok(Self(value))
    }

    fn may_contain_inline_secret(&self) -> bool {
        let lower = self.0.to_ascii_lowercase();
        (lower.contains("secret")
            || lower.contains("api_key")
            || lower.contains("password")
            || lower.ends_with(".token"))
            && !lower.ends_with("credential_ref")
            && lower != "secrets.store"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigValue {
    String(String),
    Bool(bool),
    U64(u64),
    SecretRef(p::CredentialRef),
    Strings(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLayer {
    pub schema_version: p::SchemaVersion,
    pub source: ConfigLayerSource,
    values: BTreeMap<ConfigKey, ConfigValue>,
}

impl ConfigLayer {
    pub fn new(source: ConfigLayerSource) -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            source,
            values: BTreeMap::new(),
        }
    }

    pub fn set(&mut self, key: ConfigKey, value: ConfigValue) -> p::Result<()> {
        if key.may_contain_inline_secret() && !matches!(value, ConfigValue::SecretRef(_)) {
            return Err(p::Error(
                "sensitive configuration must use a credential reference".into(),
            ));
        }
        self.values.insert(key, value);
        Ok(())
    }

    pub fn with(mut self, key: &str, value: ConfigValue) -> p::Result<Self> {
        self.set(ConfigKey::new(key)?, value)?;
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPrecedence {
    pub schema_version: p::SchemaVersion,
    pub low_to_high: Vec<ConfigLayerSource>,
}

impl Default for ConfigPrecedence {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            low_to_high: vec![
                ConfigLayerSource::Defaults,
                ConfigLayerSource::File,
                ConfigLayerSource::Environment,
                ConfigLayerSource::Cli,
            ],
        }
    }
}

impl ConfigPrecedence {
    fn validate(&self) -> p::Result<()> {
        let unique = self.low_to_high.iter().copied().collect::<BTreeSet<_>>();
        if self.schema_version.0 == 0
            || self.low_to_high.len() != 4
            || unique.len() != self.low_to_high.len()
        {
            return Err(p::Error(
                "configuration precedence must list every layer exactly once".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLayers {
    pub schema_version: p::SchemaVersion,
    pub precedence: ConfigPrecedence,
    pub layers: Vec<ConfigLayer>,
}

impl ConfigLayers {
    pub fn resolve(&self) -> p::Result<ResolvedConfig> {
        self.precedence.validate()?;
        let mut by_source = BTreeMap::new();
        for layer in &self.layers {
            if layer.schema_version.0 == 0 || by_source.insert(layer.source, layer).is_some() {
                return Err(p::Error(
                    "configuration has a duplicate or unversioned layer".into(),
                ));
            }
        }
        let mut values = BTreeMap::new();
        for source in &self.precedence.low_to_high {
            if let Some(layer) = by_source.get(source) {
                values.extend(layer.values.clone());
            }
        }
        Ok(ResolvedConfig {
            schema_version: p::SchemaVersion(1),
            values,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConfig {
    pub schema_version: p::SchemaVersion,
    values: BTreeMap<ConfigKey, ConfigValue>,
}

impl ResolvedConfig {
    pub fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.values.get(&ConfigKey(key.to_owned()))
    }

    fn string(&self, key: &str) -> p::Result<String> {
        match self.get(key) {
            Some(ConfigValue::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
            _ => Err(p::Error(format!(
                "configuration value {key} is missing or is not a string"
            ))),
        }
    }

    fn boolean(&self, key: &str) -> p::Result<bool> {
        match self.get(key) {
            Some(ConfigValue::Bool(value)) => Ok(*value),
            _ => Err(p::Error(format!(
                "configuration value {key} is missing or is not a boolean"
            ))),
        }
    }

    fn unsigned(&self, key: &str) -> p::Result<u64> {
        match self.get(key) {
            Some(ConfigValue::U64(value)) => Ok(*value),
            _ => Err(p::Error(format!(
                "configuration value {key} is missing or is not an unsigned integer"
            ))),
        }
    }

    fn secret_ref(&self, key: &str) -> p::Result<p::CredentialRef> {
        match self.get(key) {
            Some(ConfigValue::SecretRef(value)) if !value.0.trim().is_empty() => Ok(value.clone()),
            _ => Err(p::Error(format!(
                "configuration value {key} is missing or is not a credential reference"
            ))),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> p::Result<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(p::Error("secret value cannot be empty".into()));
        }
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([redacted])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SecretStore {
    pub schema_version: p::SchemaVersion,
    values: BTreeMap<p::CredentialRef, SecretValue>,
}

impl Default for SecretStore {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            values: BTreeMap::new(),
        }
    }
}

impl SecretStore {
    pub fn insert(&mut self, reference: p::CredentialRef, value: SecretValue) -> p::Result<()> {
        if reference.0.trim().is_empty() {
            return Err(p::Error("credential reference cannot be empty".into()));
        }
        self.values.insert(reference, value);
        Ok(())
    }

    pub fn contains(&self, reference: &p::CredentialRef) -> bool {
        self.values.contains_key(reference)
    }

    pub fn resolve(&self, reference: &p::CredentialRef) -> p::Result<SecretValue> {
        self.values
            .get(reference)
            .cloned()
            .ok_or_else(|| p::Error("credential reference is unresolved".into()))
    }
}

impl fmt::Debug for SecretStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretStore")
            .field("schema_version", &self.schema_version)
            .field("references", &self.values.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    pub schema_version: p::SchemaVersion,
    pub layers: ConfigLayers,
    pub secrets: SecretStore,
}

impl Config {
    pub fn new(layers: ConfigLayers, secrets: SecretStore) -> p::Result<Self> {
        layers.resolve()?;
        if secrets.schema_version.0 == 0 {
            return Err(p::Error("secret store is not versioned".into()));
        }
        Ok(Self {
            schema_version: p::SchemaVersion(1),
            layers,
            secrets,
        })
    }

    pub fn resolve(&self) -> p::Result<ResolvedConfig> {
        self.layers.resolve()
    }

    pub fn model(&self, id: &str) -> p::Result<ModelSettings> {
        let resolved = self.resolve()?;
        let prefix = format!("models.{id}");
        let context_window = resolved.unsigned(&format!("{prefix}.capability.context_window"))?;
        let context_window = u32::try_from(context_window)
            .map_err(|_| p::Error("model context window exceeds u32".into()))?;
        let requests_per_minute =
            resolved.unsigned(&format!("{prefix}.rate_limit.requests_per_minute"))?;
        let requests_per_minute = u32::try_from(requests_per_minute)
            .map_err(|_| p::Error("model request rate exceeds u32".into()))?;
        Ok(ModelSettings {
            schema_version: p::SchemaVersion(1),
            id: id.to_owned(),
            provider: resolved.string(&format!("{prefix}.provider"))?,
            model: resolved.string(&format!("{prefix}.model"))?,
            base_url: resolved.string(&format!("{prefix}.base_url"))?,
            enabled: resolved.boolean(&format!("{prefix}.enabled"))?,
            credential_ref: resolved.secret_ref(&format!("{prefix}.credential_ref"))?,
            capability: ModelCapabilitySettings {
                schema_version: p::SchemaVersion(1),
                context_window,
                tool_use: resolved.boolean(&format!("{prefix}.capability.tool_use"))?,
                strength: resolved.string(&format!("{prefix}.capability.strength"))?,
            },
            cost: CostSettings {
                schema_version: p::SchemaVersion(1),
                input_microunits_per_million: resolved
                    .unsigned(&format!("{prefix}.cost.input_microunits_per_million"))?,
                output_microunits_per_million: resolved
                    .unsigned(&format!("{prefix}.cost.output_microunits_per_million"))?,
            },
            rate_limit: RateLimitSettings {
                schema_version: p::SchemaVersion(1),
                requests_per_minute,
                tokens_per_minute: resolved
                    .unsigned(&format!("{prefix}.rate_limit.tokens_per_minute"))?,
            },
        })
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("schema_version", &self.schema_version)
            .field("layers", &self.layers)
            .field("secrets", &self.secrets)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSettings {
    pub schema_version: p::SchemaVersion,
    pub id: String,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub enabled: bool,
    pub credential_ref: p::CredentialRef,
    pub capability: ModelCapabilitySettings,
    pub cost: CostSettings,
    pub rate_limit: RateLimitSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapabilitySettings {
    pub schema_version: p::SchemaVersion,
    pub context_window: u32,
    pub tool_use: bool,
    pub strength: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostSettings {
    pub schema_version: p::SchemaVersion,
    pub input_microunits_per_million: u64,
    pub output_microunits_per_million: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitSettings {
    pub schema_version: p::SchemaVersion,
    pub requests_per_minute: u32,
    pub tokens_per_minute: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationProbe {
    pub schema_version: p::SchemaVersion,
    pub id: String,
    pub configured: bool,
    pub enabled: bool,
    pub permission_granted: bool,
    pub required: bool,
    pub allowlist_entries: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextEfficiencyProbe {
    pub schema_version: p::SchemaVersion,
    pub compaction_threshold_percent: u8,
    pub max_tokens: u64,
    pub reserve_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSystemPermission {
    pub schema_version: p::SchemaVersion,
    pub root: PathBuf,
    pub require_read: bool,
    pub require_write: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellPermission {
    pub schema_version: p::SchemaVersion,
    pub executable: PathBuf,
    pub permitted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerProbe {
    pub schema_version: p::SchemaVersion,
    pub enabled: bool,
    pub required: bool,
    pub config: p::SchedulerConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationProbe {
    pub schema_version: p::SchemaVersion,
    pub enabled: bool,
    pub required: bool,
    pub available: bool,
    pub permission_granted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalActionPostureProbe {
    pub schema_version: p::SchemaVersion,
    pub default_approval: p::ApprovalRule,
    pub l5_one_shot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserProbe {
    pub schema_version: p::SchemaVersion,
    pub enabled: bool,
    pub required: bool,
    pub registered: bool,
    pub driver_available: bool,
    pub allowed_origins: usize,
    pub artifact_root: Option<PathBuf>,
    pub secret_resolver_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputerProbe {
    pub schema_version: p::SchemaVersion,
    pub enabled: bool,
    pub required: bool,
    pub registered: bool,
    pub driver_available: bool,
    pub allowed_surfaces: usize,
    pub artifact_root: Option<PathBuf>,
    pub secret_resolver_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyProbe {
    pub schema_version: p::SchemaVersion,
    pub enabled: bool,
    pub required: bool,
    pub registered: bool,
    pub driver_available: bool,
    pub allowed_programs: Vec<PathBuf>,
    pub allowed_roots: Vec<PathBuf>,
    pub inherit_environment: bool,
    pub secret_resolver_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorProbe {
    pub schema_version: p::SchemaVersion,
    pub id: String,
    pub enabled: bool,
    pub required: bool,
    pub registered: bool,
    pub trusted: bool,
    pub grant_active: bool,
    pub driver_available: bool,
    pub schema_digest: p::SchemaDigest,
    pub requests_per_minute: u32,
    pub timeout: p::DurationMs,
    pub credential_ref: Option<p::SecretRef>,
    pub secret_resolver_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncProbe {
    pub schema_version: p::SchemaVersion,
    pub enabled: bool,
    pub required: bool,
    pub peer_configured: bool,
    pub owner_bound: bool,
    pub expected_version_cas: bool,
    pub batch_idempotency: bool,
    pub cursor_persistence: bool,
    pub redaction_policy: Option<p::SyncRedactionPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvolutionProbe {
    pub schema_version: p::SchemaVersion,
    pub enabled: bool,
    pub effect_mode: p::EffectMode,
    pub holdout_required: bool,
    pub self_eval_can_promote: bool,
    pub active_cas: bool,
    pub rollback_on_hard_regression: bool,
    pub known_good_available: bool,
    pub owner_for_bounded: bool,
    pub owner_for_expansive: bool,
    pub auto_activate_bounded: bool,
    pub artifact_root: Option<PathBuf>,
    pub secret_scanner_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAuditProbe {
    pub schema_version: p::SchemaVersion,
    pub enabled: bool,
    pub available: bool,
    pub dependency_scan: bool,
    pub license_notice_scan: bool,
    pub release_tree_scan: bool,
    pub secret_scan: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FederationProcessMode {
    Authority,
    Executor,
    Replica,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationProbe {
    pub schema_version: p::SchemaVersion,
    pub enabled: bool,
    pub required: bool,
    pub process_mode: FederationProcessMode,
    pub authority_writer_count: u8,
    pub owner_principal_configured: bool,
    pub authority_identity_configured: bool,
    pub peer_identity_configured: bool,
    pub owner_peer_identity_separate: bool,
    pub tls_1_3_only: bool,
    pub mutual_tls: bool,
    pub identity_pinned: bool,
    pub bounded_grants: bool,
    pub maximum_grant_ttl: p::DurationMs,
    pub epoch_cas: bool,
    pub lease_cas: bool,
    pub replay_ledger_persistent: bool,
    pub executor_credential_slots_local: bool,
    pub default_approval: p::ApprovalRule,
    pub l5_one_shot: bool,
    pub replica_read_only: bool,
    pub replication_redaction: bool,
    pub artifact_root: Option<PathBuf>,
    pub secret_scanner_available: bool,
    pub private_endpoint_scanner_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcosystemProbe {
    pub schema_version: p::SchemaVersion,
    pub enabled: bool,
    pub required: bool,
    pub owner_principal_configured: bool,
    pub publisher_keyring_configured: bool,
    pub catalog_allowlist_configured: bool,
    pub package_limits_configured: bool,
    pub admission_policy_configured: bool,
    pub ecosystem_cas: bool,
    pub replay_ledger_persistent: bool,
    pub executor_receiver_configured: bool,
    pub default_approval: p::ApprovalRule,
    pub executable_packages_allowed: bool,
    pub artifact_root: Option<PathBuf>,
    pub secret_scanner_available: bool,
    pub private_path_scanner_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorEnvironment {
    pub schema_version: p::SchemaVersion,
    pub model_id: String,
    pub context: ContextEfficiencyProbe,
    pub mcp_servers: Vec<IntegrationProbe>,
    pub plugins: Vec<IntegrationProbe>,
    pub file_system: Vec<FileSystemPermission>,
    pub shell: Vec<ShellPermission>,
    pub scheduler: SchedulerProbe,
    pub notification: NotificationProbe,
    pub external_actions: ExternalActionPostureProbe,
    pub browser: BrowserProbe,
    pub computer: ComputerProbe,
    pub pty: PtyProbe,
    pub connectors: Vec<ConnectorProbe>,
    pub sync: SyncProbe,
    pub evolution: EvolutionProbe,
    pub federation: FederationProbe,
    pub ecosystem: EcosystemProbe,
    pub release_audit: ReleaseAuditProbe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorCheck {
    pub schema_version: p::SchemaVersion,
    pub kind: p::ConfigCheck,
    pub ok: bool,
    pub explain: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub schema_version: p::SchemaVersion,
    pub check: p::ConfigCheck,
    pub code: String,
    pub explain: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDoctorReport {
    pub schema_version: p::SchemaVersion,
    pub checks: Vec<DoctorCheck>,
    pub findings: Vec<Finding>,
}

impl ConfigDoctorReport {
    pub fn is_ok(&self) -> bool {
        self.findings.is_empty() && self.checks.iter().all(|check| check.ok)
    }

    pub fn event_payload(&self) -> p::EventPayload {
        p::EventPayload::ConfigDoctorReport(p::ConfigDoctorReportPayload {
            checks: self.checks.iter().map(|check| check.kind).collect(),
            findings: self
                .findings
                .iter()
                .map(|finding| {
                    p::ConfigFindingRef(format!("{}: {}", finding.code, finding.explain))
                })
                .collect(),
        })
    }
}

pub trait ConfigDoctor {
    fn check(&self) -> ConfigDoctorReport;
}

pub struct RuntimeConfigDoctor<'a> {
    pub config: &'a Config,
    pub environment: &'a DoctorEnvironment,
}

impl ConfigDoctor for RuntimeConfigDoctor<'_> {
    fn check(&self) -> ConfigDoctorReport {
        let mut report = ConfigDoctorReport {
            schema_version: p::SchemaVersion(1),
            checks: Vec::new(),
            findings: Vec::new(),
        };
        let model = self.config.model(&self.environment.model_id);
        match &model {
            Ok(model) => {
                add_check(
                    &mut report,
                    p::ConfigCheck::Provider,
                    model.enabled
                        && !model.provider.trim().is_empty()
                        && !model.model.trim().is_empty()
                        && valid_http_url(&model.base_url),
                    "model provider is configured, enabled, and has a valid endpoint",
                    "provider_invalid",
                );
                add_check(
                    &mut report,
                    p::ConfigCheck::Credential,
                    self.config.secrets.contains(&model.credential_ref),
                    "model credential reference resolves in the separate secret store",
                    "credential_unresolved",
                );
                add_check(
                    &mut report,
                    p::ConfigCheck::Capability,
                    model.capability.context_window > 0
                        && matches!(
                            model.capability.strength.as_str(),
                            "basic" | "standard" | "strong"
                        )
                        && model.rate_limit.requests_per_minute > 0
                        && model.rate_limit.tokens_per_minute > 0,
                    "model capability, strength, and rate limits are internally consistent",
                    "capability_invalid",
                );
            }
            Err(error) => {
                add_check(
                    &mut report,
                    p::ConfigCheck::Provider,
                    false,
                    &format!("model provider configuration is invalid: {error}"),
                    "provider_invalid",
                );
                add_check(
                    &mut report,
                    p::ConfigCheck::Credential,
                    false,
                    "credential cannot be checked until the model configuration resolves",
                    "credential_unresolved",
                );
                add_check(
                    &mut report,
                    p::ConfigCheck::Capability,
                    false,
                    "capability cannot be checked until the model configuration resolves",
                    "capability_invalid",
                );
            }
        }
        check_integrations(
            &mut report,
            p::ConfigCheck::Mcp,
            &self.environment.mcp_servers,
            "MCP",
            true,
        );
        check_integrations(
            &mut report,
            p::ConfigCheck::Plugin,
            &self.environment.plugins,
            "plugin",
            false,
        );
        check_context(&mut report, &self.environment.context);
        check_file_system(&mut report, &self.environment.file_system);
        check_shell(&mut report, &self.environment.shell);
        check_scheduler(&mut report, &self.environment.scheduler);
        check_notification(&mut report, &self.environment.notification);
        check_browser(
            &mut report,
            &self.environment.browser,
            &self.environment.external_actions,
        );
        check_computer(
            &mut report,
            &self.environment.computer,
            &self.environment.external_actions,
        );
        check_pty(
            &mut report,
            &self.environment.pty,
            &self.environment.external_actions,
        );
        check_connectors(
            &mut report,
            &self.environment.connectors,
            &self.environment.external_actions,
        );
        check_sync(&mut report, &self.environment.sync);
        check_evolution(&mut report, &self.environment.evolution);
        check_federation(&mut report, &self.environment.federation);
        check_ecosystem(&mut report, &self.environment.ecosystem);
        check_release_audit(&mut report, &self.environment.release_audit);
        report
    }
}

fn add_check(
    report: &mut ConfigDoctorReport,
    kind: p::ConfigCheck,
    ok: bool,
    explain: &str,
    code: &str,
) {
    report.checks.push(DoctorCheck {
        schema_version: p::SchemaVersion(1),
        kind,
        ok,
        explain: explain.to_owned(),
    });
    if !ok {
        report.findings.push(Finding {
            schema_version: p::SchemaVersion(1),
            check: kind,
            code: code.to_owned(),
            explain: explain.to_owned(),
        });
    }
}

fn check_integrations(
    report: &mut ConfigDoctorReport,
    kind: p::ConfigCheck,
    probes: &[IntegrationProbe],
    label: &str,
    require_allowlist: bool,
) {
    let invalid = probes.iter().filter(|probe| {
        probe.schema_version.0 == 0
            || probe.id.trim().is_empty()
            || (probe.required
                && (!probe.configured || !probe.enabled || !probe.permission_granted))
            || (probe.enabled && (!probe.configured || !probe.permission_granted))
            || (require_allowlist && probe.enabled && probe.allowlist_entries == 0)
    });
    let names = invalid.map(|probe| probe.id.as_str()).collect::<Vec<_>>();
    let (ok, explain) = if names.is_empty() {
        (
            true,
            if require_allowlist {
                format!(
                    "{label} registrations have runtime sources, permission, and non-empty allowlists"
                )
            } else {
                format!("{label} registrations are configured, enabled, and permitted")
            },
        )
    } else {
        (
            false,
            if require_allowlist {
                format!(
                    "{label} registrations need a configured source, permission, enabled state, and non-empty allowlist: {}",
                    names.join(", ")
                )
            } else {
                format!("{label} registrations are incomplete: {}", names.join(", "))
            },
        )
    };
    add_check(
        report,
        kind,
        ok,
        &explain,
        &format!("{}_invalid", label.to_ascii_lowercase()),
    );
}

fn check_file_system(report: &mut ConfigDoctorReport, permissions: &[FileSystemPermission]) {
    let invalid = permissions
        .iter()
        .filter(|permission| {
            let Ok(metadata) = std::fs::metadata(&permission.root) else {
                return true;
            };
            permission.schema_version.0 == 0
                || !metadata.is_dir()
                || (permission.require_write && metadata.permissions().readonly())
        })
        .map(|permission| permission.root.display().to_string())
        .collect::<Vec<_>>();
    let ok = !permissions.is_empty() && invalid.is_empty();
    let explain = if permissions.is_empty() {
        "no filesystem permission roots are configured".to_owned()
    } else if invalid.is_empty() {
        "filesystem roots exist and satisfy the declared access mode".to_owned()
    } else {
        format!("filesystem roots are unavailable: {}", invalid.join(", "))
    };
    add_check(
        report,
        p::ConfigCheck::FileSystem,
        ok,
        &explain,
        "filesystem_permission_invalid",
    );
}

fn check_shell(report: &mut ConfigDoctorReport, permissions: &[ShellPermission]) {
    let invalid = permissions
        .iter()
        .filter(|permission| {
            permission.schema_version.0 == 0
                || !permission.permitted
                || !executable_exists(&permission.executable)
        })
        .map(|permission| permission.executable.display().to_string())
        .collect::<Vec<_>>();
    let ok = !permissions.is_empty() && invalid.is_empty();
    let explain = if permissions.is_empty() {
        "no shell permission or executable is configured".to_owned()
    } else if invalid.is_empty() {
        "shell executables exist and are explicitly permitted".to_owned()
    } else {
        format!("shell permissions are unavailable: {}", invalid.join(", "))
    };
    add_check(
        report,
        p::ConfigCheck::Shell,
        ok,
        &explain,
        "shell_permission_invalid",
    );
}

fn check_context(report: &mut ConfigDoctorReport, probe: &ContextEfficiencyProbe) {
    let ok = probe.schema_version.0 > 0
        && (1..=100).contains(&probe.compaction_threshold_percent)
        && probe.max_tokens > 0
        && probe.reserve_tokens < probe.max_tokens;
    let explain = if ok {
        "context compaction threshold and reserved output budget are bounded"
    } else {
        "context compaction needs a 1-100 threshold and reserve below the token limit"
    };
    add_check(
        report,
        p::ConfigCheck::Context,
        ok,
        explain,
        "context_efficiency_invalid",
    );
}

fn check_scheduler(report: &mut ConfigDoctorReport, probe: &SchedulerProbe) {
    let ok = probe.schema_version.0 > 0
        && (!probe.required || probe.enabled)
        && (!probe.enabled || probe.config.validate().is_ok());
    let explain = if ok && probe.enabled {
        "scheduler is enabled with bounded tick, lease, and claim limits"
    } else if ok {
        "scheduler is explicitly disabled and is not required"
    } else if probe.required && !probe.enabled {
        "scheduler is required but disabled"
    } else {
        "scheduler tick, lease, or maximum claims configuration is invalid"
    };
    add_check(
        report,
        p::ConfigCheck::Scheduler,
        ok,
        explain,
        "scheduler_invalid",
    );
}

fn check_notification(report: &mut ConfigDoctorReport, probe: &NotificationProbe) {
    let ok = probe.schema_version.0 > 0
        && (!probe.required || probe.enabled)
        && (!probe.enabled || (probe.available && probe.permission_granted));
    let explain = if ok && probe.enabled {
        "local notification backend is available and explicitly permitted"
    } else if ok {
        "local notification delivery is explicitly disabled and is not required"
    } else if probe.required && !probe.enabled {
        "local notification delivery is required but disabled"
    } else if !probe.available {
        "local notification backend is unavailable"
    } else {
        "local notification permission is not granted"
    };
    add_check(
        report,
        p::ConfigCheck::Notification,
        ok,
        explain,
        "notification_invalid",
    );
}

fn check_browser(
    report: &mut ConfigDoctorReport,
    probe: &BrowserProbe,
    posture: &ExternalActionPostureProbe,
) {
    let mut missing = external_lifecycle_findings(
        probe.schema_version,
        probe.enabled,
        probe.required,
        probe.registered,
        probe.driver_available,
        probe.secret_resolver_available,
        posture,
    );
    if probe.enabled && probe.allowed_origins == 0 {
        missing.push("origin allowlist");
    }
    if probe.enabled && !artifact_root_ready(probe.artifact_root.as_ref()) {
        missing.push("writable artifact root");
    }
    add_external_backend_check(
        report,
        p::ConfigCheck::Browser,
        "browser",
        probe.enabled,
        probe.required,
        probe.registered,
        missing,
    );
}

fn check_computer(
    report: &mut ConfigDoctorReport,
    probe: &ComputerProbe,
    posture: &ExternalActionPostureProbe,
) {
    let mut missing = external_lifecycle_findings(
        probe.schema_version,
        probe.enabled,
        probe.required,
        probe.registered,
        probe.driver_available,
        probe.secret_resolver_available,
        posture,
    );
    if probe.enabled && probe.allowed_surfaces == 0 {
        missing.push("surface allowlist");
    }
    if probe.enabled && !artifact_root_ready(probe.artifact_root.as_ref()) {
        missing.push("writable artifact root");
    }
    add_external_backend_check(
        report,
        p::ConfigCheck::Computer,
        "computer",
        probe.enabled,
        probe.required,
        probe.registered,
        missing,
    );
}

fn check_pty(
    report: &mut ConfigDoctorReport,
    probe: &PtyProbe,
    posture: &ExternalActionPostureProbe,
) {
    let mut missing = external_lifecycle_findings(
        probe.schema_version,
        probe.enabled,
        probe.required,
        probe.registered,
        probe.driver_available,
        probe.secret_resolver_available,
        posture,
    );
    if probe.enabled
        && (probe.allowed_programs.is_empty()
            || !probe.allowed_programs.iter().all(executable_exists))
    {
        missing.push("program allowlist");
    }
    if probe.enabled
        && (probe.allowed_roots.is_empty() || !probe.allowed_roots.iter().all(|root| root.is_dir()))
    {
        missing.push("canonical root allowlist");
    }
    if probe.enabled && probe.inherit_environment {
        missing.push("inherit_environment=false");
    }
    add_external_backend_check(
        report,
        p::ConfigCheck::Pty,
        "PTY",
        probe.enabled,
        probe.required,
        probe.registered,
        missing,
    );
}

fn check_connectors(
    report: &mut ConfigDoctorReport,
    probes: &[ConnectorProbe],
    posture: &ExternalActionPostureProbe,
) {
    let invalid = probes
        .iter()
        .filter_map(|probe| {
            let credential_invalid = probe.credential_ref.as_ref().is_some_and(|reference| {
                reference.0.trim().is_empty() || !probe.secret_resolver_available
            });
            let invalid = probe.schema_version.0 == 0
                || probe.id.trim().is_empty()
                || (probe.required && !probe.enabled)
                || (!probe.enabled && probe.registered)
                || (probe.enabled
                    && (!probe.registered
                        || !probe.trusted
                        || !probe.grant_active
                        || !probe.driver_available
                        || probe.schema_digest.0.trim().is_empty()
                        || probe.requests_per_minute == 0
                        || probe.timeout.0 == 0
                        || credential_invalid
                        || !external_posture_ok(posture)));
            invalid.then_some(probe.id.as_str())
        })
        .collect::<Vec<_>>();
    let ok = invalid.is_empty();
    let explain = if ok && probes.iter().any(|probe| probe.enabled) {
        "connectors are trusted, granted, schema-bound, rate-limited, timeout-bounded, SecretRef-resolved where needed, and governed by default ask/L5 one-shot posture".to_owned()
    } else if ok {
        "connectors are explicitly disabled or absent and are not registered".to_owned()
    } else {
        format!(
            "connector lifecycle, trust, grant, schema, rate, timeout, SecretRef, or external-action posture is incomplete: {}",
            invalid.join(", ")
        )
    };
    add_check(
        report,
        p::ConfigCheck::Connector,
        ok,
        &explain,
        "connector_invalid",
    );
}

fn check_sync(report: &mut ConfigDoctorReport, probe: &SyncProbe) {
    let redaction_ready = probe
        .redaction_policy
        .as_ref()
        .is_some_and(|policy| policy.validate().is_ok());
    let ok = probe.schema_version.0 > 0
        && (!probe.required || probe.enabled)
        && (!probe.enabled || {
            probe.peer_configured
                && probe.owner_bound
                && probe.expected_version_cas
                && probe.batch_idempotency
                && probe.cursor_persistence
                && redaction_ready
        })
        && (probe.enabled || (!probe.peer_configured && probe.redaction_policy.is_none()));
    let explain = if ok && probe.enabled {
        "sync has one owner-bound peer, expected-version/CAS, idempotent batches, persistent cursors, and SecretRef-safe redaction"
    } else if ok {
        "sync is explicitly disabled with no configured peer or transfer policy"
    } else if probe.required && !probe.enabled {
        "sync is required but disabled"
    } else {
        "sync needs one owner-bound peer, expected-version/CAS, batch idempotency, persistent cursors, and a SecretRef-safe redaction policy"
    };
    add_check(report, p::ConfigCheck::Sync, ok, explain, "sync_invalid");
}

fn check_evolution(report: &mut ConfigDoctorReport, probe: &EvolutionProbe) {
    let ready = probe.schema_version.0 > 0
        && probe.effect_mode == p::EffectMode::CounterfactualDeny
        && probe.holdout_required
        && !probe.self_eval_can_promote
        && probe.active_cas
        && probe.rollback_on_hard_regression
        && probe.known_good_available
        && probe.owner_for_bounded
        && probe.owner_for_expansive
        && !probe.auto_activate_bounded
        && artifact_root_ready(probe.artifact_root.as_ref())
        && probe.secret_scanner_available;
    let ok = !probe.enabled || ready;
    let explain = if ok && probe.enabled {
        "evolution has effect-deny, holdout, no self-promotion, CAS, known-good rollback, owner impact gates, and secret-scanned artifacts"
    } else if ok {
        "evolution is explicitly disabled; automatic activation remains unavailable"
    } else {
        "evolution needs counterfactual-deny, holdout, self-eval rejection, active CAS, known-good rollback, owner gates, bounded auto-activation disabled, writable artifacts, and secret scanning"
    };
    add_check(
        report,
        p::ConfigCheck::Evolution,
        ok,
        explain,
        "evolution_invalid",
    );
}

fn check_federation(report: &mut ConfigDoctorReport, probe: &FederationProbe) {
    const MAXIMUM_GRANT_TTL_MS: u64 = 90 * 24 * 60 * 60 * 1_000;

    let writer_count_valid = match probe.process_mode {
        FederationProcessMode::Authority => probe.authority_writer_count == 1,
        FederationProcessMode::Executor | FederationProcessMode::Replica => {
            probe.authority_writer_count == 0
        }
    };
    let ready = probe.schema_version.0 > 0
        && writer_count_valid
        && probe.owner_principal_configured
        && probe.authority_identity_configured
        && probe.peer_identity_configured
        && probe.owner_peer_identity_separate
        && probe.tls_1_3_only
        && probe.mutual_tls
        && probe.identity_pinned
        && probe.bounded_grants
        && (1..=MAXIMUM_GRANT_TTL_MS).contains(&probe.maximum_grant_ttl.0)
        && probe.epoch_cas
        && probe.lease_cas
        && probe.replay_ledger_persistent
        && probe.executor_credential_slots_local
        && probe.default_approval == p::ApprovalRule::Ask
        && probe.l5_one_shot
        && probe.replica_read_only
        && probe.replication_redaction
        && artifact_root_ready(probe.artifact_root.as_ref())
        && probe.secret_scanner_available
        && probe.private_endpoint_scanner_available;
    let ok = (!probe.required || probe.enabled) && (!probe.enabled || ready);
    let explain = if ok && probe.enabled {
        "federation has one authority writer, separate owner/peer identities, TLS 1.3 mutual authentication and pinning, bounded grants, epoch/lease CAS, persistent replay, executor-local credentials, L3/L5 governance, read-only redacted replicas, and scanned artifacts"
    } else if ok {
        "federation is explicitly disabled and is not required"
    } else if probe.required && !probe.enabled {
        "federation is required but disabled"
    } else {
        "federation needs one authority writer, separate owner/peer identities, TLS 1.3 mTLS with pinning, bounded grant TTL, epoch/lease CAS, persistent replay, executor-local credentials, default ask/L5 one-shot approval, read-only redacted replicas, writable artifacts, and secret/private-endpoint scanning"
    };
    add_check(
        report,
        p::ConfigCheck::Federation,
        ok,
        explain,
        "federation_invalid",
    );
}

fn check_release_audit(report: &mut ConfigDoctorReport, probe: &ReleaseAuditProbe) {
    let ready = probe.schema_version.0 > 0
        && probe.available
        && probe.dependency_scan
        && probe.license_notice_scan
        && probe.release_tree_scan
        && probe.secret_scan;
    let explain = if ready && probe.enabled {
        "release audit is enabled with dependency, license/NOTICE, release-tree, and secret gates"
    } else if ready {
        "release audit is available but explicitly disabled for this runtime"
    } else {
        "release audit capability is missing dependency, license/NOTICE, release-tree, or secret gates"
    };
    add_check(
        report,
        p::ConfigCheck::ReleaseAudit,
        ready,
        explain,
        "release_audit_invalid",
    );
}

fn check_ecosystem(report: &mut ConfigDoctorReport, probe: &EcosystemProbe) {
    let ready = probe.schema_version.0 > 0
        && probe.owner_principal_configured
        && probe.publisher_keyring_configured
        && probe.catalog_allowlist_configured
        && probe.package_limits_configured
        && probe.admission_policy_configured
        && probe.ecosystem_cas
        && probe.replay_ledger_persistent
        && probe.executor_receiver_configured
        && probe.default_approval == p::ApprovalRule::Ask
        && !probe.executable_packages_allowed
        && artifact_root_ready(probe.artifact_root.as_ref())
        && probe.secret_scanner_available
        && probe.private_path_scanner_available;
    let ok = (!probe.required || probe.enabled) && (!probe.enabled || ready);
    let explain = if ok && probe.enabled {
        "ecosystem has configured owner and publisher keys, allowlisted catalog, bounded declarative packages, admission/CAS/replay, executor receiver, default ask approval, and scanned artifacts"
    } else if ok {
        "ecosystem is explicitly disabled and is not required"
    } else if probe.required && !probe.enabled {
        "ecosystem is required but disabled"
    } else {
        "ecosystem needs owner and keyring configuration, catalog allowlist, package limits, admission/CAS/replay, executor receiver, default ask approval, executable denial, writable artifacts, and secret/private-path scanning"
    };
    add_check(
        report,
        p::ConfigCheck::Ecosystem,
        ok,
        explain,
        "ecosystem_invalid",
    );
}

fn external_lifecycle_findings(
    schema_version: p::SchemaVersion,
    enabled: bool,
    required: bool,
    registered: bool,
    driver_available: bool,
    secret_resolver_available: bool,
    posture: &ExternalActionPostureProbe,
) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if schema_version.0 == 0 {
        missing.push("schema version");
    }
    if required && !enabled {
        missing.push("required backend enabled state");
    }
    if !enabled && registered {
        missing.push("disabled backend non-registration");
    }
    if enabled && !registered {
        missing.push("backend registration");
    }
    if enabled && !driver_available {
        missing.push("driver availability");
    }
    if enabled && !secret_resolver_available {
        missing.push("SecretRef resolver");
    }
    if enabled && !external_posture_ok(posture) {
        missing.push("default ask and L5 one-shot approval posture");
    }
    missing
}

fn external_posture_ok(posture: &ExternalActionPostureProbe) -> bool {
    posture.schema_version.0 > 0
        && posture.default_approval == p::ApprovalRule::Ask
        && posture.l5_one_shot
}

fn artifact_root_ready(root: Option<&PathBuf>) -> bool {
    root.and_then(|root| std::fs::metadata(root).ok())
        .is_some_and(|metadata| metadata.is_dir() && !metadata.permissions().readonly())
}

fn add_external_backend_check(
    report: &mut ConfigDoctorReport,
    kind: p::ConfigCheck,
    label: &str,
    enabled: bool,
    required: bool,
    registered: bool,
    missing: Vec<&str>,
) {
    let ok = missing.is_empty();
    let explain = if ok && enabled {
        format!(
            "{label} backend is registered with driver, allowlists, SecretRef resolver, artifact boundary where applicable, and default ask/L5 one-shot governance"
        )
    } else if ok && !required && !registered {
        format!("{label} backend is explicitly disabled and not registered")
    } else {
        format!(
            "{label} backend configuration is incomplete: {}",
            missing.join(", ")
        )
    };
    add_check(
        report,
        kind,
        ok,
        &explain,
        &format!("{}_invalid", label.to_ascii_lowercase()),
    );
}

fn executable_exists(executable: &PathBuf) -> bool {
    if executable.components().count() > 1 || executable.is_absolute() {
        return executable.is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|root| {
                let direct = root.join(executable);
                direct.is_file()
                    || cfg!(windows)
                        && ["exe", "cmd", "bat"]
                            .iter()
                            .any(|extension| direct.with_extension(extension).is_file())
            })
        })
        .unwrap_or(false)
}

fn valid_http_url(value: &str) -> bool {
    (value.starts_with("https://") || value.starts_with("http://"))
        && value.len() > "https://".len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_layer(source: ConfigLayerSource, model: &str) -> ConfigLayer {
        let prefix = "models.primary";
        ConfigLayer::new(source)
            .with(
                &format!("{prefix}.provider"),
                ConfigValue::String("provider-a".into()),
            )
            .unwrap()
            .with(
                &format!("{prefix}.model"),
                ConfigValue::String(model.into()),
            )
            .unwrap()
            .with(
                &format!("{prefix}.base_url"),
                ConfigValue::String("https://models.invalid/v1".into()),
            )
            .unwrap()
            .with(&format!("{prefix}.enabled"), ConfigValue::Bool(true))
            .unwrap()
            .with(
                &format!("{prefix}.credential_ref"),
                ConfigValue::SecretRef(p::CredentialRef("secret:model".into())),
            )
            .unwrap()
            .with(
                &format!("{prefix}.capability.context_window"),
                ConfigValue::U64(16_384),
            )
            .unwrap()
            .with(
                &format!("{prefix}.capability.tool_use"),
                ConfigValue::Bool(true),
            )
            .unwrap()
            .with(
                &format!("{prefix}.capability.strength"),
                ConfigValue::String("strong".into()),
            )
            .unwrap()
            .with(
                &format!("{prefix}.cost.input_microunits_per_million"),
                ConfigValue::U64(1),
            )
            .unwrap()
            .with(
                &format!("{prefix}.cost.output_microunits_per_million"),
                ConfigValue::U64(2),
            )
            .unwrap()
            .with(
                &format!("{prefix}.rate_limit.requests_per_minute"),
                ConfigValue::U64(60),
            )
            .unwrap()
            .with(
                &format!("{prefix}.rate_limit.tokens_per_minute"),
                ConfigValue::U64(100_000),
            )
            .unwrap()
    }

    fn config(layers: Vec<ConfigLayer>, include_secret: bool) -> Config {
        let mut secrets = SecretStore::default();
        if include_secret {
            secrets
                .insert(
                    p::CredentialRef("secret:model".into()),
                    SecretValue::new("private-value").unwrap(),
                )
                .unwrap();
        }
        Config::new(
            ConfigLayers {
                schema_version: p::SchemaVersion(1),
                precedence: ConfigPrecedence::default(),
                layers,
            },
            secrets,
        )
        .unwrap()
    }

    #[test]
    fn precedence_is_defaults_then_file_then_environment_then_cli() {
        let config = config(
            vec![
                model_layer(ConfigLayerSource::Defaults, "default-model"),
                model_layer(ConfigLayerSource::File, "file-model"),
                model_layer(ConfigLayerSource::Environment, "env-model"),
                model_layer(ConfigLayerSource::Cli, "cli-model"),
            ],
            true,
        );
        assert_eq!(config.model("primary").unwrap().model, "cli-model");
    }

    #[test]
    fn secrets_are_references_and_debug_output_is_redacted() {
        let config = config(
            vec![model_layer(ConfigLayerSource::File, "safe-model")],
            true,
        );
        let debug = format!("{config:?}");
        assert!(debug.contains("secret:model"));
        assert!(!debug.contains("private-value"));
        assert!(ConfigLayer::new(ConfigLayerSource::File)
            .with(
                "models.primary.api_key",
                ConfigValue::String("inline".into())
            )
            .is_err());
    }

    #[test]
    fn doctor_checks_every_required_matrix_row_with_explanations() {
        let config = config(
            vec![model_layer(ConfigLayerSource::File, "safe-model")],
            true,
        );
        let root = std::env::temp_dir();
        let executable = std::env::current_exe().unwrap();
        let environment = DoctorEnvironment {
            schema_version: p::SchemaVersion(1),
            model_id: "primary".into(),
            context: ContextEfficiencyProbe {
                schema_version: p::SchemaVersion(1),
                compaction_threshold_percent: 80,
                max_tokens: 16_384,
                reserve_tokens: 2_048,
            },
            mcp_servers: vec![IntegrationProbe {
                schema_version: p::SchemaVersion(1),
                id: "mcp-a".into(),
                configured: true,
                enabled: true,
                permission_granted: true,
                required: true,
                allowlist_entries: 1,
            }],
            plugins: vec![IntegrationProbe {
                schema_version: p::SchemaVersion(1),
                id: "plugin-a".into(),
                configured: true,
                enabled: true,
                permission_granted: true,
                required: true,
                allowlist_entries: 0,
            }],
            file_system: vec![FileSystemPermission {
                schema_version: p::SchemaVersion(1),
                root: root.clone(),
                require_read: true,
                require_write: false,
            }],
            shell: vec![ShellPermission {
                schema_version: p::SchemaVersion(1),
                executable: executable.clone(),
                permitted: true,
            }],
            scheduler: SchedulerProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                required: true,
                config: p::SchedulerConfig {
                    schema_version: p::SchemaVersion(1),
                    tick: p::DurationMs(1_000),
                    lease: p::DurationMs(30_000),
                    max_claims_per_tick: 1,
                },
            },
            notification: NotificationProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                required: true,
                available: true,
                permission_granted: true,
            },
            external_actions: ExternalActionPostureProbe {
                schema_version: p::SchemaVersion(1),
                default_approval: p::ApprovalRule::Ask,
                l5_one_shot: true,
            },
            browser: BrowserProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                required: true,
                registered: true,
                driver_available: true,
                allowed_origins: 1,
                artifact_root: Some(root.clone()),
                secret_resolver_available: true,
            },
            computer: ComputerProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                required: true,
                registered: true,
                driver_available: true,
                allowed_surfaces: 1,
                artifact_root: Some(root.clone()),
                secret_resolver_available: true,
            },
            pty: PtyProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                required: true,
                registered: true,
                driver_available: true,
                allowed_programs: vec![executable],
                allowed_roots: vec![root],
                inherit_environment: false,
                secret_resolver_available: true,
            },
            connectors: vec![ConnectorProbe {
                schema_version: p::SchemaVersion(1),
                id: "connector:project-loopback".into(),
                enabled: true,
                required: true,
                registered: true,
                trusted: true,
                grant_active: true,
                driver_available: true,
                schema_digest: p::SchemaDigest("sha256:project-loopback-v1".into()),
                requests_per_minute: 4,
                timeout: p::DurationMs(2_000),
                credential_ref: Some(p::SecretRef("secret:connector".into())),
                secret_resolver_available: true,
            }],
            sync: SyncProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                required: true,
                peer_configured: true,
                owner_bound: true,
                expected_version_cas: true,
                batch_idempotency: true,
                cursor_persistence: true,
                redaction_policy: Some(p::SyncRedactionPolicy {
                    schema_version: p::SchemaVersion(1),
                    reference: p::RedactionPolicyRef("sync-redaction:doctor".into()),
                    redact_raw_content: true,
                    forbid_secret_refs: true,
                    forbidden_keys: vec!["authorization".into()],
                    forbidden_value_markers: vec!["secret:".into()],
                }),
            },
            evolution: EvolutionProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                effect_mode: p::EffectMode::CounterfactualDeny,
                holdout_required: true,
                self_eval_can_promote: false,
                active_cas: true,
                rollback_on_hard_regression: true,
                known_good_available: true,
                owner_for_bounded: true,
                owner_for_expansive: true,
                auto_activate_bounded: false,
                artifact_root: Some(std::env::temp_dir()),
                secret_scanner_available: true,
            },
            federation: FederationProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                required: true,
                process_mode: FederationProcessMode::Authority,
                authority_writer_count: 1,
                owner_principal_configured: true,
                authority_identity_configured: true,
                peer_identity_configured: true,
                owner_peer_identity_separate: true,
                tls_1_3_only: true,
                mutual_tls: true,
                identity_pinned: true,
                bounded_grants: true,
                maximum_grant_ttl: p::DurationMs(86_400_000),
                epoch_cas: true,
                lease_cas: true,
                replay_ledger_persistent: true,
                executor_credential_slots_local: true,
                default_approval: p::ApprovalRule::Ask,
                l5_one_shot: true,
                replica_read_only: true,
                replication_redaction: true,
                artifact_root: Some(std::env::temp_dir()),
                secret_scanner_available: true,
                private_endpoint_scanner_available: true,
            },
            ecosystem: EcosystemProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                required: true,
                owner_principal_configured: true,
                publisher_keyring_configured: true,
                catalog_allowlist_configured: true,
                package_limits_configured: true,
                admission_policy_configured: true,
                ecosystem_cas: true,
                replay_ledger_persistent: true,
                executor_receiver_configured: true,
                default_approval: p::ApprovalRule::Ask,
                executable_packages_allowed: false,
                artifact_root: Some(std::env::temp_dir()),
                secret_scanner_available: true,
                private_path_scanner_available: true,
            },
            release_audit: ReleaseAuditProbe {
                schema_version: p::SchemaVersion(1),
                enabled: false,
                available: true,
                dependency_scan: true,
                license_notice_scan: true,
                release_tree_scan: true,
                secret_scan: true,
            },
        };
        let report = RuntimeConfigDoctor {
            config: &config,
            environment: &environment,
        }
        .check();
        assert!(report.is_ok(), "{:#?}", report.findings);
        assert_eq!(report.checks.len(), 19);
        assert!(report
            .checks
            .iter()
            .all(|check| check.ok && !check.explain.is_empty()));
        assert!(matches!(
            report.event_payload(),
            p::EventPayload::ConfigDoctorReport(_)
        ));

        let disabled = DoctorEnvironment {
            browser: BrowserProbe {
                schema_version: p::SchemaVersion(1),
                enabled: false,
                required: false,
                registered: false,
                driver_available: false,
                allowed_origins: 0,
                artifact_root: None,
                secret_resolver_available: false,
            },
            computer: ComputerProbe {
                schema_version: p::SchemaVersion(1),
                enabled: false,
                required: false,
                registered: false,
                driver_available: false,
                allowed_surfaces: 0,
                artifact_root: None,
                secret_resolver_available: false,
            },
            pty: PtyProbe {
                schema_version: p::SchemaVersion(1),
                enabled: false,
                required: false,
                registered: false,
                driver_available: false,
                allowed_programs: Vec::new(),
                allowed_roots: Vec::new(),
                inherit_environment: false,
                secret_resolver_available: false,
            },
            connectors: vec![ConnectorProbe {
                schema_version: p::SchemaVersion(1),
                id: "connector:disabled".into(),
                enabled: false,
                required: false,
                registered: false,
                trusted: false,
                grant_active: false,
                driver_available: false,
                schema_digest: p::SchemaDigest(String::new()),
                requests_per_minute: 0,
                timeout: p::DurationMs(0),
                credential_ref: None,
                secret_resolver_available: false,
            }],
            sync: SyncProbe {
                schema_version: p::SchemaVersion(1),
                enabled: false,
                required: false,
                peer_configured: false,
                owner_bound: false,
                expected_version_cas: false,
                batch_idempotency: false,
                cursor_persistence: false,
                redaction_policy: None,
            },
            ..environment.clone()
        };
        let disabled_report = RuntimeConfigDoctor {
            config: &config,
            environment: &disabled,
        }
        .check();
        assert!(disabled_report.is_ok(), "{:#?}", disabled_report.findings);
        for kind in [
            p::ConfigCheck::Browser,
            p::ConfigCheck::Computer,
            p::ConfigCheck::Pty,
            p::ConfigCheck::Connector,
        ] {
            assert!(disabled_report.checks.iter().any(|check| {
                check.kind == kind && check.ok && check.explain.contains("not registered")
            }));
        }
        assert!(disabled_report.checks.iter().any(|check| {
            check.kind == p::ConfigCheck::Sync
                && check.ok
                && check.explain.contains("explicitly disabled")
        }));
    }

    #[test]
    fn doctor_reports_mismatch_instead_of_deferring_to_runtime() {
        let config = config(
            vec![model_layer(ConfigLayerSource::File, "safe-model")],
            false,
        );
        let environment = DoctorEnvironment {
            schema_version: p::SchemaVersion(1),
            model_id: "primary".into(),
            context: ContextEfficiencyProbe {
                schema_version: p::SchemaVersion(1),
                compaction_threshold_percent: 0,
                max_tokens: 100,
                reserve_tokens: 100,
            },
            mcp_servers: vec![IntegrationProbe {
                schema_version: p::SchemaVersion(1),
                id: "required-mcp".into(),
                configured: false,
                enabled: false,
                permission_granted: false,
                required: true,
                allowlist_entries: 0,
            }],
            plugins: Vec::new(),
            file_system: Vec::new(),
            shell: Vec::new(),
            scheduler: SchedulerProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                required: true,
                config: p::SchedulerConfig {
                    schema_version: p::SchemaVersion(1),
                    tick: p::DurationMs(0),
                    lease: p::DurationMs(30_000),
                    max_claims_per_tick: 1,
                },
            },
            notification: NotificationProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                required: true,
                available: true,
                permission_granted: false,
            },
            external_actions: ExternalActionPostureProbe {
                schema_version: p::SchemaVersion(1),
                default_approval: p::ApprovalRule::Allow,
                l5_one_shot: false,
            },
            browser: BrowserProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                required: true,
                registered: false,
                driver_available: false,
                allowed_origins: 0,
                artifact_root: None,
                secret_resolver_available: false,
            },
            computer: ComputerProbe {
                schema_version: p::SchemaVersion(1),
                enabled: false,
                required: true,
                registered: false,
                driver_available: false,
                allowed_surfaces: 0,
                artifact_root: None,
                secret_resolver_available: false,
            },
            pty: PtyProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                required: true,
                registered: true,
                driver_available: true,
                allowed_programs: Vec::new(),
                allowed_roots: Vec::new(),
                inherit_environment: true,
                secret_resolver_available: false,
            },
            connectors: vec![ConnectorProbe {
                schema_version: p::SchemaVersion(1),
                id: "connector:incomplete".into(),
                enabled: true,
                required: true,
                registered: true,
                trusted: false,
                grant_active: false,
                driver_available: true,
                schema_digest: p::SchemaDigest(String::new()),
                requests_per_minute: 0,
                timeout: p::DurationMs(0),
                credential_ref: Some(p::SecretRef("secret:connector".into())),
                secret_resolver_available: false,
            }],
            sync: SyncProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                required: true,
                peer_configured: false,
                owner_bound: false,
                expected_version_cas: false,
                batch_idempotency: false,
                cursor_persistence: false,
                redaction_policy: Some(p::SyncRedactionPolicy {
                    schema_version: p::SchemaVersion(1),
                    reference: p::RedactionPolicyRef("sync-redaction:invalid".into()),
                    redact_raw_content: false,
                    forbid_secret_refs: false,
                    forbidden_keys: Vec::new(),
                    forbidden_value_markers: Vec::new(),
                }),
            },
            evolution: EvolutionProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                effect_mode: p::EffectMode::LiveGoverned,
                holdout_required: false,
                self_eval_can_promote: true,
                active_cas: false,
                rollback_on_hard_regression: false,
                known_good_available: false,
                owner_for_bounded: false,
                owner_for_expansive: false,
                auto_activate_bounded: true,
                artifact_root: None,
                secret_scanner_available: false,
            },
            federation: FederationProbe {
                schema_version: p::SchemaVersion(0),
                enabled: true,
                required: true,
                process_mode: FederationProcessMode::Executor,
                authority_writer_count: 1,
                owner_principal_configured: false,
                authority_identity_configured: false,
                peer_identity_configured: false,
                owner_peer_identity_separate: false,
                tls_1_3_only: false,
                mutual_tls: false,
                identity_pinned: false,
                bounded_grants: false,
                maximum_grant_ttl: p::DurationMs(0),
                epoch_cas: false,
                lease_cas: false,
                replay_ledger_persistent: false,
                executor_credential_slots_local: false,
                default_approval: p::ApprovalRule::Allow,
                l5_one_shot: false,
                replica_read_only: false,
                replication_redaction: false,
                artifact_root: None,
                secret_scanner_available: false,
                private_endpoint_scanner_available: false,
            },
            ecosystem: EcosystemProbe {
                schema_version: p::SchemaVersion(0),
                enabled: true,
                required: true,
                owner_principal_configured: false,
                publisher_keyring_configured: false,
                catalog_allowlist_configured: false,
                package_limits_configured: false,
                admission_policy_configured: false,
                ecosystem_cas: false,
                replay_ledger_persistent: false,
                executor_receiver_configured: false,
                default_approval: p::ApprovalRule::Allow,
                executable_packages_allowed: true,
                artifact_root: None,
                secret_scanner_available: false,
                private_path_scanner_available: false,
            },
            release_audit: ReleaseAuditProbe {
                schema_version: p::SchemaVersion(1),
                enabled: true,
                available: false,
                dependency_scan: false,
                license_notice_scan: false,
                release_tree_scan: false,
                secret_scan: false,
            },
        };
        let report = RuntimeConfigDoctor {
            config: &config,
            environment: &environment,
        }
        .check();
        assert!(!report.is_ok());
        let failed = report
            .findings
            .iter()
            .map(|finding| finding.check)
            .collect::<Vec<_>>();
        assert!(failed.contains(&p::ConfigCheck::Credential));
        assert!(failed.contains(&p::ConfigCheck::Context));
        assert!(failed.contains(&p::ConfigCheck::Mcp));
        assert!(failed.contains(&p::ConfigCheck::FileSystem));
        assert!(failed.contains(&p::ConfigCheck::Shell));
        assert!(failed.contains(&p::ConfigCheck::Scheduler));
        assert!(failed.contains(&p::ConfigCheck::Notification));
        assert!(failed.contains(&p::ConfigCheck::Browser));
        assert!(failed.contains(&p::ConfigCheck::Computer));
        assert!(failed.contains(&p::ConfigCheck::Pty));
        assert!(failed.contains(&p::ConfigCheck::Connector));
        assert!(failed.contains(&p::ConfigCheck::Sync));
        assert!(failed.contains(&p::ConfigCheck::Evolution));
        assert!(failed.contains(&p::ConfigCheck::Federation));
        assert!(failed.contains(&p::ConfigCheck::Ecosystem));
        assert!(failed.contains(&p::ConfigCheck::ReleaseAudit));
        assert!(report.findings.iter().any(|finding| {
            finding.code == "scheduler_invalid" && finding.explain.contains("tick")
        }));
        assert!(report.findings.iter().any(|finding| {
            finding.code == "notification_invalid" && finding.explain.contains("permission")
        }));
        assert!(report.findings.iter().any(|finding| {
            finding.code == "context_efficiency_invalid" && finding.explain.contains("reserve")
        }));
        assert!(report.findings.iter().any(|finding| {
            finding.code == "mcp_invalid" && finding.explain.contains("allowlist")
        }));
        assert!(report.findings.iter().any(|finding| {
            finding.code == "browser_invalid"
                && finding.explain.contains("L5 one-shot")
                && finding.explain.contains("origin allowlist")
        }));
        assert!(report.findings.iter().any(|finding| {
            finding.code == "computer_invalid" && finding.explain.contains("required")
        }));
        assert!(report.findings.iter().any(|finding| {
            finding.code == "pty_invalid"
                && finding.explain.contains("program allowlist")
                && finding.explain.contains("inherit_environment=false")
        }));
        assert!(report.findings.iter().any(|finding| {
            finding.code == "connector_invalid"
                && finding.explain.contains("schema")
                && finding.explain.contains("SecretRef")
        }));
        assert!(report.findings.iter().any(|finding| {
            finding.code == "sync_invalid"
                && finding.explain.contains("expected-version/CAS")
                && finding.explain.contains("SecretRef-safe redaction")
        }));
        assert!(report.findings.iter().any(|finding| {
            finding.code == "evolution_invalid"
                && finding.explain.contains("counterfactual-deny")
                && finding.explain.contains("owner gates")
        }));
        assert!(report.findings.iter().any(|finding| {
            finding.code == "release_audit_invalid"
                && finding.explain.contains("license/NOTICE")
                && finding.explain.contains("secret")
        }));
    }
}
