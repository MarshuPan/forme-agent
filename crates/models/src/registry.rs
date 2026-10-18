//! Deterministic production model registration and A-wave selection.

use std::{collections::BTreeMap, fmt, sync::Arc};

use forme_protocol as p;
use serde::Deserialize;

use crate::{
    ChatCompletionsProvider, Cost, ModelCapability, ModelProfile, ModelProvider, ModelStrength,
    RateLimit, SecretString, ToolCompatibility, Url,
};

const MAX_PROFILE_CONFIG_BYTES: usize = 65_536;
const MAX_MODEL_PROFILES: usize = 32;
const MAX_CONFIG_STRING_BYTES: usize = 2_048;

#[derive(Clone)]
pub struct RegisteredModel {
    reference: p::ModelProfileRef,
    profile: ModelProfile,
    provider: Arc<dyn ModelProvider>,
    fallback_group: Option<String>,
    tool_compatibility: Vec<ToolCompatibility>,
}

impl RegisteredModel {
    pub fn new(
        reference: p::ModelProfileRef,
        profile: ModelProfile,
        provider: Arc<dyn ModelProvider>,
        fallback_group: Option<String>,
        mut tool_compatibility: Vec<ToolCompatibility>,
    ) -> p::Result<Self> {
        profile.validate()?;
        required(&reference.0, "model profile reference")?;
        if provider.profile() != profile {
            return Err(p::Error(
                "registered model provider profile does not match its declaration".into(),
            ));
        }
        if fallback_group
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(p::Error("model fallback group is empty".into()));
        }
        for compatibility in &tool_compatibility {
            if compatibility.schema_version.0 == 0
                || compatibility.model_profile != reference
                || compatibility.tool_schema_version.0 == 0
            {
                return Err(p::Error(
                    "model tool compatibility does not match the registered profile".into(),
                ));
            }
        }
        tool_compatibility.sort_by_key(|entry| entry.tool_schema_version);
        if tool_compatibility
            .windows(2)
            .any(|entries| entries[0].tool_schema_version == entries[1].tool_schema_version)
        {
            return Err(p::Error(
                "model tool compatibility has duplicate schema versions".into(),
            ));
        }
        Ok(Self {
            reference,
            profile,
            provider,
            fallback_group,
            tool_compatibility,
        })
    }

    pub fn reference(&self) -> &p::ModelProfileRef {
        &self.reference
    }

    pub fn profile(&self) -> &ModelProfile {
        &self.profile
    }

    pub fn provider(&self) -> Arc<dyn ModelProvider> {
        self.provider.clone()
    }

    pub fn fallback_group(&self) -> Option<&str> {
        self.fallback_group.as_deref()
    }

    fn permits_tools(&self, schema: Option<p::Version>) -> bool {
        if !self.profile.capability.tool_use {
            return false;
        }
        match schema {
            Some(schema) => self
                .tool_compatibility
                .iter()
                .any(|entry| entry.compatible && entry.tool_schema_version == schema),
            None => true,
        }
    }
}

impl fmt::Debug for RegisteredModel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RegisteredModel")
            .field("reference", &self.reference)
            .field("profile", &self.profile)
            .field("fallback_group", &self.fallback_group)
            .field("tool_compatibility", &self.tool_compatibility)
            .field("provider", &"<bound>")
            .finish()
    }
}

#[derive(Clone)]
pub struct ModelRegistry {
    models: BTreeMap<p::ModelProfileRef, RegisteredModel>,
    default: p::ModelProfileRef,
}

impl ModelRegistry {
    pub fn new(default: p::ModelProfileRef) -> p::Result<Self> {
        required(&default.0, "default model profile reference")?;
        Ok(Self {
            models: BTreeMap::new(),
            default,
        })
    }

    pub fn from_environment() -> p::Result<Self> {
        Self::from_config(ModelRegistryConfig::from_environment()?)
    }

    /// Builds a registry from an already resolved, secret-free configuration.
    /// Credential values are resolved only while binding the concrete provider
    /// at this boundary; the config itself contains only the environment
    /// variable name.
    pub fn from_config(config: ModelRegistryConfig) -> p::Result<Self> {
        config.validate()?;
        let defaults = config
            .profiles
            .iter()
            .filter(|profile| profile.default)
            .map(|profile| p::ModelProfileRef(profile.id.clone()))
            .collect::<Vec<_>>();
        if defaults.len() != 1 {
            return Err(p::Error(
                "model configuration must declare exactly one default profile".into(),
            ));
        }
        let mut registry = Self::new(defaults[0].clone())?;
        for profile in config.profiles {
            registry.register(profile.bind_provider()?)?;
        }
        registry.validate_ready()?;
        Ok(registry)
    }

    /// Like [`ModelRegistry::from_config`], but a non-default profile whose
    /// provider cannot be bound (missing credential, unusable endpoint) is
    /// left out and reported instead of failing the whole registry. The
    /// default profile is required: its failure still fails closed.
    pub fn from_config_degrading(
        config: ModelRegistryConfig,
    ) -> p::Result<(Self, Vec<UnavailableModelProfile>)> {
        config.validate()?;
        let defaults = config
            .profiles
            .iter()
            .filter(|profile| profile.default)
            .map(|profile| p::ModelProfileRef(profile.id.clone()))
            .collect::<Vec<_>>();
        if defaults.len() != 1 {
            return Err(p::Error(
                "model configuration must declare exactly one default profile".into(),
            ));
        }
        let mut registry = Self::new(defaults[0].clone())?;
        let mut unavailable = Vec::new();
        for profile in config.profiles {
            let reference = p::ModelProfileRef(profile.id.clone());
            let is_default = profile.default;
            match profile.bind_provider() {
                Ok(model) => registry.register(model)?,
                Err(error) if is_default => return Err(error),
                Err(_) => unavailable.push(UnavailableModelProfile {
                    profile: reference,
                    reason: "model provider binding is unavailable (credential or endpoint)".into(),
                }),
            }
        }
        registry.validate_ready()?;
        Ok((registry, unavailable))
    }

    pub fn register(&mut self, model: RegisteredModel) -> p::Result<()> {
        if self.models.contains_key(model.reference()) {
            return Err(p::Error(format!(
                "duplicate model profile reference: {}",
                model.reference().0
            )));
        }
        self.models.insert(model.reference().clone(), model);
        Ok(())
    }

    pub fn validate_ready(&self) -> p::Result<()> {
        if self.models.is_empty() || !self.models.contains_key(&self.default) {
            return Err(p::Error(
                "model registry has no registered default profile".into(),
            ));
        }
        Ok(())
    }

    pub fn default_ref(&self) -> &p::ModelProfileRef {
        &self.default
    }

    pub fn get(&self, reference: &p::ModelProfileRef) -> Option<&RegisteredModel> {
        self.models.get(reference)
    }

    /// Resolves one attempt to the concrete provider registered for its
    /// profile.  Callers must bind the returned provider before emitting the
    /// corresponding ModelCallStarted event.
    pub fn provider(&self, reference: &p::ModelProfileRef) -> p::Result<Arc<dyn ModelProvider>> {
        self.get(reference)
            .map(RegisteredModel::provider)
            .ok_or_else(|| p::Error(format!("model profile is not registered: {}", reference.0)))
    }

    pub fn profiles(&self) -> Vec<p::ModelProfileRef> {
        self.models.keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.models.len()
    }

    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }
}

/// A configured, non-default model profile that could not be bound. The
/// reason is a fixed, secret-free explanation for the doctor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnavailableModelProfile {
    pub profile: p::ModelProfileRef,
    pub reason: String,
}

/// Secret-free model configuration used by the production composition root.
/// The actual credential is intentionally not representable here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRegistryConfig {
    pub schema_version: p::SchemaVersion,
    pub profiles: Vec<ModelRuntimeConfig>,
}

impl ModelRegistryConfig {
    pub fn from_environment() -> p::Result<Self> {
        let profiles = match std::env::var("FORME_MODEL_PROFILES_JSON") {
            Ok(value) if !value.trim().is_empty() => parse_profile_config(&value)?,
            _ => vec![legacy_environment_profile()?],
        };
        let config = Self {
            schema_version: p::SchemaVersion(1),
            profiles,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || self.profiles.is_empty()
            || self.profiles.len() > MAX_MODEL_PROFILES
        {
            return Err(p::Error("model registry configuration is invalid".into()));
        }
        let mut ids = std::collections::BTreeSet::new();
        for profile in &self.profiles {
            profile.validate()?;
            if !ids.insert(profile.id.clone()) {
                return Err(p::Error(format!(
                    "duplicate model profile reference: {}",
                    profile.id
                )));
            }
        }
        Ok(())
    }
}

impl fmt::Debug for ModelRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModelRegistry")
            .field("profiles", &self.profiles())
            .field("default", &self.default)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelectionRequest {
    pub schema_version: p::SchemaVersion,
    pub task: Option<p::GoalFrameRef>,
    pub minimum_context_window: u32,
    pub require_tool_use: bool,
    pub tool_schema_version: Option<p::Version>,
    pub minimum_strength: ModelStrength,
    pub risk: p::Risk,
    pub evidence: Vec<p::EvidenceRef>,
}

impl ModelSelectionRequest {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0 || self.minimum_context_window == 0 {
            return Err(p::Error("model selection request is incomplete".into()));
        }
        if self
            .task
            .as_ref()
            .is_some_and(|task| task.0.trim().is_empty())
            || self
                .tool_schema_version
                .is_some_and(|version| version.0 == 0)
            || self.evidence.iter().any(|value| value.0.trim().is_empty())
        {
            return Err(p::Error(
                "model selection request contains an invalid reference".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelection {
    pub schema_version: p::SchemaVersion,
    pub selected: p::ModelProfileRef,
    pub policy: p::ReasonRef,
    pub rationale: p::Rationale,
    pub fallback_chain: Vec<p::ModelProfileRef>,
}

pub trait ModelSelector: Send + Sync {
    fn select(&self, request: &ModelSelectionRequest) -> p::Result<ModelSelection>;
}

impl ModelSelector for ModelRegistry {
    fn select(&self, request: &ModelSelectionRequest) -> p::Result<ModelSelection> {
        request.validate()?;
        self.validate_ready()?;
        let mut candidates = self
            .models
            .values()
            .filter(|registered| {
                let profile = registered.profile();
                profile.capability.context_window >= request.minimum_context_window
                    && strength_rank(profile.capability.strength)
                        >= strength_rank(request.minimum_strength)
                    && (!request.require_tool_use
                        || registered.permits_tools(request.tool_schema_version))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|registered| {
            let profile = registered.profile();
            (
                strength_rank(profile.capability.strength),
                profile.cost.input_microunits_per_million,
                profile.cost.output_microunits_per_million,
                registered.reference().clone(),
            )
        });
        let selected = candidates
            .first()
            .copied()
            .ok_or_else(|| p::Error("no compatible model profile is registered".into()))?;
        let fallback_chain = candidates
            .iter()
            .skip(1)
            .filter(|candidate| candidate.fallback_group() == selected.fallback_group())
            .map(|candidate| candidate.reference().clone())
            .collect::<Vec<_>>();
        Ok(ModelSelection {
            schema_version: p::SchemaVersion(1),
            selected: selected.reference().clone(),
            policy: p::ReasonRef("model-selection:a-wave-deterministic".into()),
            rationale: p::Rationale(format!(
                "compatible profile selected for context={}, tools={}, risk={:?}, evidence={}",
                request.minimum_context_window,
                request.require_tool_use,
                request.risk,
                request.evidence.len()
            )),
            fallback_chain,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRuntimeConfig {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub credential_env: String,
    #[serde(default = "default_context_window")]
    pub context_window: u32,
    #[serde(default)]
    pub tool_use: bool,
    #[serde(default = "default_strength")]
    pub strength: ModelStrength,
    #[serde(default)]
    pub input_microunits_per_million: u64,
    #[serde(default)]
    pub output_microunits_per_million: u64,
    #[serde(default = "default_requests_per_minute")]
    pub requests_per_minute: u32,
    #[serde(default = "default_tokens_per_minute")]
    pub tokens_per_minute: u64,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub fallback_group: Option<String>,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub tool_schema_versions: Vec<u32>,
}

impl ModelRuntimeConfig {
    pub fn validate(&self) -> p::Result<()> {
        for (value, name) in [
            (&self.id, "model profile id"),
            (&self.provider, "model provider"),
            (&self.model, "model name"),
            (&self.base_url, "model base URL"),
            (
                &self.credential_env,
                "model credential environment variable",
            ),
        ] {
            required(value, name)?;
            if value.len() > MAX_CONFIG_STRING_BYTES {
                return Err(p::Error(format!("{name} exceeds its bound")));
            }
        }
        if !self
            .credential_env
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(p::Error(
                "model credential environment variable name is invalid".into(),
            ));
        }
        if self.context_window == 0
            || self.requests_per_minute == 0
            || self.tokens_per_minute == 0
            || self.timeout_ms == 0
            || self.tool_schema_versions.contains(&0)
        {
            return Err(p::Error("model profile limits are invalid".into()));
        }
        let mut schemas = self.tool_schema_versions.clone();
        schemas.sort_unstable();
        schemas.dedup();
        if schemas.len() != self.tool_schema_versions.len() {
            return Err(p::Error("model tool schema versions must be unique".into()));
        }
        Url::parse(self.base_url.clone())?;
        Ok(())
    }

    fn bind_provider(self) -> p::Result<RegisteredModel> {
        self.validate()?;
        let credential = std::env::var(&self.credential_env).map_err(|_| {
            p::Error(format!(
                "credential for model profile {} is unavailable",
                self.id
            ))
        })?;
        let reference = p::ModelProfileRef(self.id.clone());
        let profile = ModelProfile {
            schema_version: p::SchemaVersion(1),
            provider: p::ProviderId(self.provider),
            model: self.model,
            base_url: Url::parse(self.base_url)?,
            capability: ModelCapability {
                schema_version: p::SchemaVersion(1),
                context_window: self.context_window,
                tool_use: self.tool_use,
                strength: self.strength,
            },
            cost: Cost {
                schema_version: p::SchemaVersion(1),
                input_microunits_per_million: self.input_microunits_per_million,
                output_microunits_per_million: self.output_microunits_per_million,
            },
            rate_limit: RateLimit {
                schema_version: p::SchemaVersion(1),
                requests_per_minute: self.requests_per_minute,
                tokens_per_minute: self.tokens_per_minute,
            },
            credential_ref: p::CredentialRef(format!("env:{}", self.credential_env)),
        };
        let provider = Arc::new(ChatCompletionsProvider::new(
            profile.clone(),
            SecretString::new(credential)?,
            p::DurationMs(self.timeout_ms),
        )?);
        let compatibility = self
            .tool_schema_versions
            .into_iter()
            .map(|version| ToolCompatibility {
                schema_version: p::SchemaVersion(1),
                model_profile: reference.clone(),
                tool_schema_version: p::Version(version),
                compatible: true,
                reason: None,
            })
            .collect();
        RegisteredModel::new(
            reference,
            profile,
            provider,
            self.fallback_group,
            compatibility,
        )
    }
}

fn parse_profile_config(value: &str) -> p::Result<Vec<ModelRuntimeConfig>> {
    if value.len() > MAX_PROFILE_CONFIG_BYTES {
        return Err(p::Error(
            "model profile configuration exceeds its bound".into(),
        ));
    }
    let profiles: Vec<ModelRuntimeConfig> = serde_json::from_str(value)
        .map_err(|error| p::Error(format!("model profile configuration is invalid: {error}")))?;
    if profiles.is_empty() || profiles.len() > MAX_MODEL_PROFILES {
        return Err(p::Error(
            "model profile configuration count is invalid".into(),
        ));
    }
    for profile in &profiles {
        profile.validate()?;
    }
    Ok(profiles)
}

fn legacy_environment_profile() -> p::Result<ModelRuntimeConfig> {
    Ok(ModelRuntimeConfig {
        id: std::env::var("FORME_MODEL_PROFILE_ID").unwrap_or_else(|_| {
            let provider = std::env::var("FORME_MODEL_PROVIDER")
                .unwrap_or_else(|_| "configured-chat-provider".into());
            let model = required_env("FORME_MODEL_NAME").unwrap_or_else(|_| "missing".into());
            format!("{provider}:{model}")
        }),
        provider: std::env::var("FORME_MODEL_PROVIDER")
            .unwrap_or_else(|_| "configured-chat-provider".into()),
        model: required_env("FORME_MODEL_NAME")?,
        base_url: required_env("FORME_MODEL_BASE_URL")?,
        credential_env: "FORME_MODEL_API_KEY".into(),
        context_window: optional_env("FORME_MODEL_CONTEXT_WINDOW", default_context_window())?,
        tool_use: false,
        strength: ModelStrength::Standard,
        input_microunits_per_million: 0,
        output_microunits_per_million: 0,
        requests_per_minute: 60,
        tokens_per_minute: 1_000_000,
        timeout_ms: u64::from(optional_env("FORME_MODEL_TIMEOUT_MS", 120_000)?),
        fallback_group: Some("legacy-default".into()),
        default: true,
        tool_schema_versions: Vec::new(),
    })
}

fn required_env(name: &str) -> p::Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| p::Error(format!("required environment variable {name} is missing")))
}

fn optional_env(name: &str, default: u32) -> p::Result<u32> {
    match std::env::var(name) {
        Ok(value) => value
            .parse::<u32>()
            .map_err(|_| p::Error(format!("environment variable {name} is invalid"))),
        Err(_) => Ok(default),
    }
}

fn required(value: &str, name: &str) -> p::Result<()> {
    if value.trim().is_empty() {
        Err(p::Error(format!("{name} is incomplete")))
    } else {
        Ok(())
    }
}

const fn strength_rank(strength: ModelStrength) -> u8 {
    match strength {
        ModelStrength::Basic => 0,
        ModelStrength::Standard => 1,
        ModelStrength::Strong => 2,
    }
}

const fn default_context_window() -> u32 {
    32_768
}

const fn default_requests_per_minute() -> u32 {
    60
}

const fn default_tokens_per_minute() -> u64 {
    1_000_000
}

const fn default_timeout_ms() -> u64 {
    120_000
}

const fn default_strength() -> ModelStrength {
    ModelStrength::Standard
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModelOutput, ModelResponse, ScriptedModelProvider};

    fn profile(id: &str, strength: ModelStrength, tools: bool, cost: u64) -> RegisteredModel {
        let reference = p::ModelProfileRef(id.into());
        let profile = ModelProfile {
            schema_version: p::SchemaVersion(1),
            provider: p::ProviderId("test".into()),
            model: id.into(),
            base_url: Url::parse("https://models.invalid/v1").unwrap(),
            capability: ModelCapability {
                schema_version: p::SchemaVersion(1),
                context_window: 32_768,
                tool_use: tools,
                strength,
            },
            cost: Cost {
                schema_version: p::SchemaVersion(1),
                input_microunits_per_million: cost,
                output_microunits_per_million: cost,
            },
            rate_limit: RateLimit {
                schema_version: p::SchemaVersion(1),
                requests_per_minute: 60,
                tokens_per_minute: 100_000,
            },
            credential_ref: p::CredentialRef("env:TEST_ONLY_KEY".into()),
        };
        let provider = Arc::new(
            ScriptedModelProvider::new(
                profile.clone(),
                vec![ModelResponse {
                    schema_version: p::SchemaVersion(1),
                    output: ModelOutput::Final("ok".into()),
                    usage: p::ModelUsage {
                        input_tokens: 1,
                        output_tokens: 1,
                    },
                    finish_reason: p::FinishReason("stop".into()),
                }],
            )
            .unwrap(),
        );
        let compatibility = tools
            .then(|| ToolCompatibility {
                schema_version: p::SchemaVersion(1),
                model_profile: reference.clone(),
                tool_schema_version: p::Version(1),
                compatible: true,
                reason: None,
            })
            .into_iter()
            .collect();
        RegisteredModel::new(
            reference,
            profile,
            provider,
            Some("default".into()),
            compatibility,
        )
        .unwrap()
    }

    #[test]
    fn registry_rejects_duplicates_and_selects_deterministically() {
        let mut registry = ModelRegistry::new(p::ModelProfileRef("standard".into())).unwrap();
        registry
            .register(profile("standard", ModelStrength::Standard, true, 20))
            .unwrap();
        registry
            .register(profile("strong", ModelStrength::Strong, true, 10))
            .unwrap();
        assert!(registry
            .register(profile("strong", ModelStrength::Strong, true, 10))
            .is_err());
        let selection = registry
            .select(&ModelSelectionRequest {
                schema_version: p::SchemaVersion(1),
                task: Some(p::GoalFrameRef("task:test".into())),
                minimum_context_window: 8_000,
                require_tool_use: true,
                tool_schema_version: Some(p::Version(1)),
                minimum_strength: ModelStrength::Standard,
                risk: p::Risk::Medium,
                evidence: vec![p::EvidenceRef("evidence:test".into())],
            })
            .unwrap();
        assert_eq!(selection.selected, p::ModelProfileRef("standard".into()));
        assert_eq!(
            selection.fallback_chain,
            vec![p::ModelProfileRef("strong".into())]
        );
    }

    #[test]
    fn tool_incompatibility_is_a_blocking_selection_error() {
        let mut registry = ModelRegistry::new(p::ModelProfileRef("text".into())).unwrap();
        registry
            .register(profile("text", ModelStrength::Standard, false, 1))
            .unwrap();
        let error = registry
            .select(&ModelSelectionRequest {
                schema_version: p::SchemaVersion(1),
                task: None,
                minimum_context_window: 1,
                require_tool_use: true,
                tool_schema_version: Some(p::Version(1)),
                minimum_strength: ModelStrength::Basic,
                risk: p::Risk::Low,
                evidence: Vec::new(),
            })
            .unwrap_err();
        assert!(error.0.contains("no compatible"));
    }

    #[test]
    fn json_config_is_bounded_closed_and_keeps_credentials_indirect() {
        let json = r#"[{
            "id":"primary",
            "provider":"compatible",
            "model":"model-a",
            "base_url":"https://models.invalid/v1",
            "credential_env":"FORME_TEST_MODEL_KEY",
            "default":true
        }]"#;
        let profiles = parse_profile_config(json).unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].credential_env, "FORME_TEST_MODEL_KEY");
        assert!(!json.contains("actual-secret-value"));
        assert!(parse_profile_config(
            r#"[{"id":"x","provider":"p","model":"m","base_url":"https://x.invalid","credential_env":"KEY","default":true,"unknown":1}]"#
        )
        .is_err());
    }

    fn runtime_profile(id: &str, credential_env: &str, default: bool) -> ModelRuntimeConfig {
        ModelRuntimeConfig {
            id: id.into(),
            provider: "compatible".into(),
            model: format!("model-{id}"),
            base_url: "https://models.invalid/v1".into(),
            credential_env: credential_env.into(),
            context_window: 32_768,
            tool_use: false,
            strength: ModelStrength::Standard,
            input_microunits_per_million: 0,
            output_microunits_per_million: 0,
            requests_per_minute: 60,
            tokens_per_minute: 100_000,
            timeout_ms: 1_000,
            fallback_group: Some("group".into()),
            default,
            tool_schema_versions: Vec::new(),
        }
    }

    #[test]
    fn optional_profile_binding_failure_degrades_but_default_fails_closed() {
        std::env::set_var("FORME_REGISTRY_DEGRADE_PRESENT", "registry-test-value");
        std::env::remove_var("FORME_REGISTRY_DEGRADE_ABSENT");
        let (registry, unavailable) = ModelRegistry::from_config_degrading(ModelRegistryConfig {
            schema_version: p::SchemaVersion(1),
            profiles: vec![
                runtime_profile("primary", "FORME_REGISTRY_DEGRADE_PRESENT", true),
                runtime_profile("fallback", "FORME_REGISTRY_DEGRADE_ABSENT", false),
            ],
        })
        .unwrap();
        assert_eq!(
            registry.profiles(),
            vec![p::ModelProfileRef("primary".into())]
        );
        assert_eq!(
            unavailable
                .iter()
                .map(|profile| profile.profile.0.as_str())
                .collect::<Vec<_>>(),
            vec!["fallback"]
        );
        assert!(!format!("{unavailable:?}").contains("FORME_REGISTRY_DEGRADE_ABSENT"));

        assert!(ModelRegistry::from_config_degrading(ModelRegistryConfig {
            schema_version: p::SchemaVersion(1),
            profiles: vec![
                runtime_profile("primary", "FORME_REGISTRY_DEGRADE_ABSENT", true),
                runtime_profile("fallback", "FORME_REGISTRY_DEGRADE_PRESENT", false),
            ],
        })
        .is_err());
        // The strict constructor keeps its fail-closed contract.
        assert!(ModelRegistry::from_config(ModelRegistryConfig {
            schema_version: p::SchemaVersion(1),
            profiles: vec![
                runtime_profile("primary", "FORME_REGISTRY_DEGRADE_PRESENT", true),
                runtime_profile("fallback", "FORME_REGISTRY_DEGRADE_ABSENT", false),
            ],
        })
        .is_err());
    }
}
