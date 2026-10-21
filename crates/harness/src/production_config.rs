//! Typed, secret-free configuration for the production capability surface.
//!
//! The composition root owns this configuration because it is the only place
//! where capability discovery, backend construction, policy, and context
//! visibility can be assembled as one lifecycle.  Runtime secrets are
//! represented by environment-variable names and resolved only by the
//! execution driver that needs them.

#![forbid(unsafe_code)]

use std::fmt;
use std::path::{Path, PathBuf};

use forme_capabilities::{LocalPlugin, SkillDefinition, StdioMcpServer};
use forme_policy::ComputerSurfaceLimit;
use forme_protocol as p;
use serde::Deserialize;

mod document;
use document::{load_capability_document, parse_capability_document};

/// Owner-authored capability document read from `FORME_CAPABILITY_CONFIG`.
///
/// Version 1 is the first formal-path source for providers that cannot be
/// described by flat environment variables.  It carries environment-variable
/// names for secrets, never secret values.
pub const CAPABILITY_CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRuntimeConfig {
    pub schema_version: p::SchemaVersion,
    /// Canonical budget identity for the production capability envelope.
    ///
    /// Provider grants use an opaque budget reference, so unlike numeric
    /// limits they cannot be safely widened or merged. Every active provider
    /// must bind to this exact value or remain unavailable.
    pub envelope_budget: p::Budget,
    pub output_budget_bytes: u64,
    pub timeout: p::DurationMs,
    pub file: FileRuntimeConfig,
    pub shell: ShellRuntimeConfig,
    pub browser: BrowserRuntimeConfig,
    pub computer: ComputerRuntimeConfig,
    pub pty: PtyRuntimeConfig,
    pub notification_enabled: bool,
    pub mcp: Vec<McpRuntimeConfig>,
    pub skills: Vec<SkillDefinition>,
    pub plugins: Vec<PluginRuntimeConfig>,
    pub app_api: Vec<AppApiRuntimeConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRuntimeConfig {
    pub enabled: bool,
    pub roots: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellRuntimeConfig {
    pub enabled: bool,
    pub allowed_programs: Vec<String>,
    pub allowed_roots: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub network_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserRuntimeConfig {
    pub enabled: bool,
    pub driver: p::ProviderId,
    pub executable: Option<PathBuf>,
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputerRuntimeConfig {
    pub enabled: bool,
    pub driver: p::ProviderId,
    pub surfaces: Vec<ComputerSurfaceLimit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyRuntimeConfig {
    pub enabled: bool,
    pub allowed_programs: Vec<PathBuf>,
    pub allowed_roots: Vec<PathBuf>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct McpRuntimeConfig {
    pub server: StdioMcpServer,
    /// Names of runtime environment variables copied into the child's
    /// otherwise empty environment during assembly.  Values are resolved at
    /// the composition boundary so configuration files never hold them.
    pub inherit_env: Vec<String>,
    pub enabled: bool,
    pub trust: Option<p::TrustTier>,
    pub grant: Option<p::AutonomyEnvelope>,
    pub discover_on_start: bool,
}

impl fmt::Debug for McpRuntimeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Child environment values may be credentials. Only their names are
        // diagnostic; the executable location stays out of public output.
        formatter
            .debug_struct("McpRuntimeConfig")
            .field("provider_id", &self.server.provider_id)
            .field(
                "env_names",
                &self
                    .server
                    .env
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>(),
            )
            .field("inherit_env", &self.inherit_env)
            .field("allowlist", &self.server.allowlist)
            .field("enabled", &self.enabled)
            .field("trust", &self.trust)
            .field("grant", &self.grant.is_some())
            .field("discover_on_start", &self.discover_on_start)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRuntimeConfig {
    pub plugin: LocalPlugin,
    pub enabled: bool,
    pub trust: Option<p::TrustTier>,
    pub grant: Option<p::AutonomyEnvelope>,
    pub activate_on_start: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppApiRuntimeConfig {
    pub provider_id: p::ProviderId,
    pub base_url: String,
    pub schema_digest: p::SchemaDigest,
    pub credential_env: Option<String>,
    pub scope: p::Scope,
    pub capability: p::CapabilityRef,
    pub permission: p::PermissionRef,
    pub allowed_mutations: Vec<p::AppApiMutationMethod>,
    pub requests_per_minute: u32,
    pub timeout: p::DurationMs,
    pub max_response_bytes: u64,
    pub enabled: bool,
    pub trust: Option<p::TrustTier>,
    pub grant: Option<p::AutonomyEnvelope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CognitiveRuntimeConfig {
    pub relevance_mode: p::RelevanceMode,
    pub schema_version: p::SchemaVersion,
    pub tick_enabled: bool,
    /// Cadence of the owner-brain tick driven by the shared scheduler pass.
    pub tick_interval: p::DurationMs,
    /// How long stable cognition may go without its evidence being
    /// reproduced before the tick's maintenance phase marks it for
    /// re-evaluation.
    pub stable_evidence_max_age: p::DurationMs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataProtectionConfig {
    pub schema_version: p::SchemaVersion,
    pub artifact_root: PathBuf,
}

impl Default for CapabilityRuntimeConfig {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            envelope_budget: p::Budget("units:100000".into()),
            output_budget_bytes: 64 * 1024,
            timeout: p::DurationMs(120_000),
            file: FileRuntimeConfig {
                enabled: false,
                roots: Vec::new(),
            },
            shell: ShellRuntimeConfig {
                enabled: false,
                allowed_programs: Vec::new(),
                allowed_roots: Vec::new(),
                environment: Vec::new(),
                network_allowed: false,
            },
            browser: BrowserRuntimeConfig {
                enabled: false,
                driver: p::ProviderId("browser:chrome".into()),
                executable: None,
                allowed_origins: Vec::new(),
            },
            computer: ComputerRuntimeConfig {
                enabled: false,
                driver: p::ProviderId("computer:native".into()),
                surfaces: Vec::new(),
            },
            pty: PtyRuntimeConfig {
                enabled: false,
                allowed_programs: Vec::new(),
                allowed_roots: Vec::new(),
            },
            notification_enabled: true,
            mcp: Vec::new(),
            skills: Vec::new(),
            plugins: Vec::new(),
            app_api: Vec::new(),
        }
    }
}

/// Thirty days without reproduction before stable cognition is re-evaluated.
const DEFAULT_STABLE_EVIDENCE_MAX_AGE_MS: u64 = 30 * 24 * 60 * 60 * 1_000;

impl Default for CognitiveRuntimeConfig {
    fn default() -> Self {
        Self {
            relevance_mode: p::RelevanceMode::DirectModel,
            schema_version: p::SchemaVersion(1),
            tick_enabled: true,
            tick_interval: p::DurationMs(60_000),
            stable_evidence_max_age: p::DurationMs(DEFAULT_STABLE_EVIDENCE_MAX_AGE_MS),
        }
    }
}

impl Default for DataProtectionConfig {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            artifact_root: PathBuf::from(".forme").join("artifacts"),
        }
    }
}

impl CapabilityRuntimeConfig {
    pub fn from_environment() -> p::Result<Self> {
        let mut config = Self::default();
        config.envelope_budget = p::Budget(
            std::env::var("FORME_CAPABILITY_BUDGET")
                .unwrap_or_else(|_| config.envelope_budget.0.clone()),
        );
        config.output_budget_bytes =
            optional_u64("FORME_CAPABILITY_OUTPUT_BYTES", config.output_budget_bytes)?;
        config.timeout = p::DurationMs(optional_u64(
            "FORME_CAPABILITY_TIMEOUT_MS",
            config.timeout.0,
        )?);
        config.notification_enabled = optional_bool("FORME_NOTIFICATION_ENABLED", true)?;

        let file_roots = optional_list("FORME_FILE_ROOTS")?;
        config.file = FileRuntimeConfig {
            enabled: optional_bool("FORME_FILE_ENABLED", !file_roots.is_empty())?,
            roots: file_roots,
        };

        let shell_programs = optional_list("FORME_SHELL_PROGRAMS")?;
        let shell_roots = optional_list("FORME_SHELL_ROOTS")?;
        config.shell = ShellRuntimeConfig {
            enabled: optional_bool("FORME_SHELL_ENABLED", false)?,
            allowed_programs: shell_programs,
            allowed_roots: shell_roots,
            environment: Vec::new(),
            network_allowed: optional_bool("FORME_SHELL_NETWORK", false)?,
        };

        let browser_executable = std::env::var_os("FORME_BROWSER_EXECUTABLE").map(PathBuf::from);
        let browser_origins = optional_list("FORME_BROWSER_ALLOWED_ORIGINS")?;
        config.browser = BrowserRuntimeConfig {
            enabled: optional_bool("FORME_BROWSER_ENABLED", browser_executable.is_some())?,
            driver: p::ProviderId(
                std::env::var("FORME_BROWSER_DRIVER")
                    .unwrap_or_else(|_| "browser:chrome".to_owned()),
            ),
            executable: browser_executable,
            allowed_origins: browser_origins,
        };

        let pty_programs = optional_path_list("FORME_PTY_PROGRAMS")?;
        let pty_roots = optional_path_list("FORME_PTY_ROOTS")?;
        config.pty = PtyRuntimeConfig {
            enabled: optional_bool("FORME_PTY_ENABLED", false)?,
            allowed_programs: pty_programs,
            allowed_roots: pty_roots,
        };
        config.computer.enabled = optional_bool("FORME_COMPUTER_ENABLED", false)?;

        // External providers are never inferred from arbitrary environment
        // variables. They enter through the owner's typed capability document
        // so every provider carries an explicit enable/trust/grant lifecycle.
        if let Some(path) = std::env::var_os("FORME_CAPABILITY_CONFIG") {
            load_capability_document(Path::new(&path))?.apply(&mut config)?;
        }
        config.validate()?;
        Ok(config)
    }

    /// Applies an owner-authored capability document to this configuration.
    /// The document replaces provider lists wholesale so a partially applied
    /// file can never leave stale providers behind.
    pub fn apply_capability_document(&mut self, document: &str) -> p::Result<()> {
        parse_capability_document(document)?.apply(self)
    }

    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || self.envelope_budget.0.trim().is_empty()
            || self.output_budget_bytes == 0
            || self.timeout.0 == 0
        {
            return Err(p::Error(
                "capability runtime configuration is incomplete or invalid".into(),
            ));
        }
        // Optional capabilities are availability-checked by the production
        // builder. Missing roots, drivers, schemas, credentials, or limits
        // produce Disabled/Unsupported doctor rows and never make the owner
        // brain itself unbuildable or visible as a half-configured tool.
        for (name, _) in &self.shell.environment {
            if !valid_environment_name(name) {
                return Err(p::Error("shell environment name is invalid".into()));
            }
        }
        for server in &self.mcp {
            if server.server.provider_id.0.trim().is_empty() {
                return Err(p::Error("MCP server identity is missing".into()));
            }
            if server
                .server
                .env
                .iter()
                .map(|(name, _)| name)
                .chain(&server.inherit_env)
                .any(|name| !valid_child_environment_name(name))
            {
                return Err(p::Error("MCP environment name is invalid".into()));
            }
        }
        for plugin in &self.plugins {
            if plugin.plugin.manifest.id.0.trim().is_empty() {
                return Err(p::Error("plugin identity is missing".into()));
            }
        }
        for app_api in &self.app_api {
            required_string(&app_api.provider_id.0, "App API provider")?;
            if app_api
                .credential_env
                .as_ref()
                .is_some_and(|name| !valid_environment_name(name))
            {
                return Err(p::Error(
                    "App API credential environment name is invalid".into(),
                ));
            }
        }
        Ok(())
    }
}

impl CognitiveRuntimeConfig {
    pub fn from_environment() -> p::Result<Self> {
        let config = Self {
            relevance_mode: match std::env::var("FORME_RELEVANCE_MODE").as_deref() {
                Ok("direct-model") | Err(std::env::VarError::NotPresent) => {
                    p::RelevanceMode::DirectModel
                }
                Ok("diffusion") => p::RelevanceMode::Diffusion,
                _ => return Err(p::Error("FORME_RELEVANCE_MODE is invalid".into())),
            },
            schema_version: p::SchemaVersion(1),
            tick_enabled: optional_bool("FORME_COGNITIVE_TICK_ENABLED", true)?,
            tick_interval: p::DurationMs(optional_u64("FORME_COGNITIVE_TICK_MS", 60_000)?),
            stable_evidence_max_age: p::DurationMs(optional_u64(
                "FORME_STABLE_EVIDENCE_MAX_AGE_MS",
                DEFAULT_STABLE_EVIDENCE_MAX_AGE_MS,
            )?),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || self.tick_interval.0 == 0
            || i64::try_from(self.tick_interval.0).is_err()
            || self.stable_evidence_max_age.0 == 0
            || i64::try_from(self.stable_evidence_max_age.0).is_err()
        {
            return Err(p::Error(
                "cognitive runtime configuration is invalid".into(),
            ));
        }
        Ok(())
    }
}

impl DataProtectionConfig {
    pub fn from_environment() -> p::Result<Self> {
        let config = Self {
            schema_version: p::SchemaVersion(1),
            artifact_root: std::env::var_os("FORME_ARTIFACT_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".forme").join("artifacts")),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0 || self.artifact_root.as_os_str().is_empty() {
            return Err(p::Error("data protection configuration is invalid".into()));
        }
        Ok(())
    }
}

fn optional_list(name: &str) -> p::Result<Vec<String>> {
    match std::env::var(name) {
        Ok(value) => Ok(value
            .split(';')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
            .collect()),
        Err(std::env::VarError::NotPresent) => Ok(Vec::new()),
        Err(std::env::VarError::NotUnicode(_)) => Err(p::Error(format!(
            "environment variable {name} is invalid UTF-8"
        ))),
    }
}

fn optional_path_list(name: &str) -> p::Result<Vec<PathBuf>> {
    optional_list(name).map(|values| values.into_iter().map(PathBuf::from).collect())
}

fn optional_u64(name: &str, fallback: u64) -> p::Result<u64> {
    match std::env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|_| p::Error(format!("environment variable {name} is invalid"))),
        Err(std::env::VarError::NotPresent) => Ok(fallback),
        Err(std::env::VarError::NotUnicode(_)) => Err(p::Error(format!(
            "environment variable {name} is invalid UTF-8"
        ))),
    }
}

fn optional_bool(name: &str, fallback: bool) -> p::Result<bool> {
    match std::env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err(p::Error(format!("environment variable {name} is invalid"))),
        },
        Err(std::env::VarError::NotPresent) => Ok(fallback),
        Err(std::env::VarError::NotUnicode(_)) => Err(p::Error(format!(
            "environment variable {name} is invalid UTF-8"
        ))),
    }
}

fn required_string(value: &str, label: &str) -> p::Result<()> {
    if value.trim().is_empty() {
        Err(p::Error(format!("{label} is empty")))
    } else {
        Ok(())
    }
}

fn valid_environment_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Child-process variables follow platform spelling (for example Windows'
/// mixed-case system variables) but still exclude separators and `=`.
fn valid_child_environment_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.as_bytes()[0].is_ascii_digit()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'(' | b')'))
}
