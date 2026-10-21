//! Closed capability document parsing and atomic conversion to runtime configuration.
use super::*;
use forme_capabilities::{
    Capability, CapabilityDescriptor, McpAllowlist, PluginContribution, PluginManifest, SkillBody,
    SkillMetadata,
};
use std::io::Read;

const MAX_CAPABILITY_CONFIG_BYTES: u64 = 262_144;
const MAX_CONFIGURED_SKILLS: usize = 128;
const MAX_CONFIGURED_PROVIDERS: usize = 32;
const MAX_CONFIG_TEXT_BYTES: usize = 16_384;

pub(super) fn load_capability_document(path: &Path) -> p::Result<CapabilityConfigDocument> {
    // The path itself is private owner data; errors describe the failure
    // class without echoing it.
    let file = std::fs::File::open(path)
        .map_err(|_| p::Error("capability configuration document is unreadable".into()))?;
    let mut contents = String::new();
    file.take(MAX_CAPABILITY_CONFIG_BYTES + 1)
        .read_to_string(&mut contents)
        .map_err(|_| p::Error("capability configuration document is unreadable".into()))?;
    parse_capability_document(&contents)
}

pub(super) fn parse_capability_document(contents: &str) -> p::Result<CapabilityConfigDocument> {
    if contents.len() as u64 > MAX_CAPABILITY_CONFIG_BYTES {
        return Err(p::Error(
            "capability configuration document exceeds its bound".into(),
        ));
    }
    let document: CapabilityConfigDocument = serde_json::from_str(contents).map_err(|error| {
        p::Error(format!(
            "capability configuration document is invalid at line {} column {}",
            error.line(),
            error.column()
        ))
    })?;
    document.validate()?;
    Ok(document)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CapabilityConfigDocument {
    schema_version: u32,
    #[serde(default)]
    skills: Vec<SkillConfigEntry>,
    #[serde(default)]
    mcp: Vec<McpConfigEntry>,
    #[serde(default)]
    plugins: Vec<PluginConfigEntry>,
    #[serde(default)]
    app_api: Vec<AppApiConfigEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillConfigEntry {
    id: p::SkillRef,
    summary: String,
    scope: p::Scope,
    #[serde(default = "first_version")]
    version: p::Version,
    /// Omitted trust is the conservative default: a configured skill body is
    /// data until the owner explicitly marks it as their own input.
    #[serde(default)]
    trust: p::TrustTier,
    body: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentEntry {
    name: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct McpAllowlistEntry {
    #[serde(default)]
    tools: Vec<String>,
    #[serde(default)]
    resources: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct McpConfigEntry {
    provider_id: p::ProviderId,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: Vec<EnvironmentEntry>,
    #[serde(default)]
    inherit_env: Vec<String>,
    allowlist: McpAllowlistEntry,
    timeout: p::DurationMs,
    declared_capabilities: p::CapabilitySet,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    trust: Option<p::TrustTier>,
    #[serde(default)]
    grant: Option<p::AutonomyEnvelope>,
    #[serde(default)]
    discover_on_start: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginToolEntry {
    id: p::CapabilityRef,
    tool: p::ToolRef,
    scope: p::Scope,
    #[serde(default)]
    permissions: Vec<p::PermissionRef>,
    #[serde(default)]
    risk: Option<p::Risk>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginConfigEntry {
    id: p::PluginRef,
    manifest_ref: p::PluginManifestRef,
    #[serde(default)]
    skills: Vec<SkillConfigEntry>,
    #[serde(default)]
    tools: Vec<PluginToolEntry>,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    trust: Option<p::TrustTier>,
    #[serde(default)]
    grant: Option<p::AutonomyEnvelope>,
    #[serde(default)]
    activate_on_start: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AppApiConfigEntry {
    provider_id: p::ProviderId,
    base_url: String,
    schema_digest: p::SchemaDigest,
    #[serde(default)]
    credential_env: Option<String>,
    scope: p::Scope,
    capability: p::CapabilityRef,
    permission: p::PermissionRef,
    #[serde(default)]
    allowed_mutations: Vec<p::AppApiMutationMethod>,
    requests_per_minute: u32,
    timeout: p::DurationMs,
    max_response_bytes: u64,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    trust: Option<p::TrustTier>,
    #[serde(default)]
    grant: Option<p::AutonomyEnvelope>,
}

fn first_version() -> p::Version {
    p::Version(1)
}

impl CapabilityConfigDocument {
    fn validate(&self) -> p::Result<()> {
        let plugin_skills = self
            .plugins
            .iter()
            .map(|plugin| plugin.skills.len())
            .sum::<usize>();
        if self.schema_version != CAPABILITY_CONFIG_SCHEMA_VERSION
            || self.skills.len() + plugin_skills > MAX_CONFIGURED_SKILLS
            || self.mcp.len() > MAX_CONFIGURED_PROVIDERS
            || self.plugins.len() > MAX_CONFIGURED_PROVIDERS
            || self.app_api.len() > MAX_CONFIGURED_PROVIDERS
        {
            return Err(p::Error(
                "capability configuration document version or size is unsupported".into(),
            ));
        }
        for skill in self
            .skills
            .iter()
            .chain(self.plugins.iter().flat_map(|plugin| &plugin.skills))
        {
            bounded_text(&skill.id.0, "skill identity")?;
            bounded_text(&skill.summary, "skill summary")?;
            bounded_text(&skill.scope.0, "skill scope")?;
            bounded_text(&skill.body, "skill body")?;
            if skill.version.0 == 0 {
                return Err(p::Error("skill version is invalid".into()));
            }
        }
        for server in &self.mcp {
            bounded_text(&server.provider_id.0, "MCP server identity")?;
            bounded_text(&server.command, "MCP server command")?;
            if server.timeout.0 == 0 || server.allowlist.tools.is_empty() {
                return Err(p::Error(
                    "MCP server timeout or tool allowlist is missing".into(),
                ));
            }
            for entry in &server.env {
                if !valid_child_environment_name(&entry.name) {
                    return Err(p::Error("MCP environment name is invalid".into()));
                }
            }
        }
        for plugin in &self.plugins {
            bounded_text(&plugin.id.0, "plugin identity")?;
            bounded_text(&plugin.manifest_ref.0, "plugin manifest reference")?;
            if plugin.skills.is_empty() && plugin.tools.is_empty() {
                return Err(p::Error("plugin declares no contribution".into()));
            }
            for tool in &plugin.tools {
                bounded_text(&tool.id.0, "plugin tool capability")?;
                bounded_text(&tool.tool.0, "plugin tool reference")?;
                bounded_text(&tool.scope.0, "plugin tool scope")?;
            }
        }
        for connector in &self.app_api {
            bounded_text(&connector.provider_id.0, "App API provider")?;
            bounded_text(&connector.base_url, "App API base URL")?;
            if connector.requests_per_minute == 0
                || connector.timeout.0 == 0
                || connector.max_response_bytes == 0
            {
                return Err(p::Error("App API connector limits are invalid".into()));
            }
        }
        Ok(())
    }

    pub(super) fn apply(self, config: &mut CapabilityRuntimeConfig) -> p::Result<()> {
        config.skills = self.skills.into_iter().map(skill_definition).collect();
        config.mcp = self
            .mcp
            .into_iter()
            .map(|entry| McpRuntimeConfig {
                server: StdioMcpServer {
                    schema_version: p::SchemaVersion(1),
                    provider_id: entry.provider_id,
                    command: entry.command,
                    args: entry.args,
                    env: entry
                        .env
                        .into_iter()
                        .map(|variable| (variable.name, variable.value))
                        .collect(),
                    allowlist: McpAllowlist {
                        schema_version: p::SchemaVersion(1),
                        tools: entry.allowlist.tools,
                        resources: entry.allowlist.resources,
                    },
                    timeout: entry.timeout,
                    declared_capabilities: entry.declared_capabilities,
                },
                inherit_env: entry.inherit_env,
                enabled: entry.enabled,
                trust: entry.trust,
                grant: entry.grant,
                discover_on_start: entry.discover_on_start,
            })
            .collect();
        config.plugins = self
            .plugins
            .into_iter()
            .map(|entry| {
                let mut contributions = entry
                    .skills
                    .into_iter()
                    .map(|skill| PluginContribution::Skill(skill_definition(skill)))
                    .collect::<Vec<_>>();
                contributions.extend(entry.tools.into_iter().map(|tool| {
                    PluginContribution::Tool(CapabilityDescriptor {
                        schema_version: p::SchemaVersion(1),
                        id: tool.id,
                        capability: Capability::Tool(tool.tool),
                        scope: tool.scope,
                        permissions: tool.permissions,
                        risk: tool.risk,
                        enabled: true,
                    })
                }));
                PluginRuntimeConfig {
                    plugin: LocalPlugin {
                        schema_version: p::SchemaVersion(1),
                        manifest: PluginManifest {
                            schema_version: p::SchemaVersion(1),
                            id: entry.id,
                            manifest_ref: entry.manifest_ref,
                            // A manifest cannot declare its own trust; the
                            // owner binding below is the only trust source.
                            trust: p::TrustTier::Untrusted,
                            contributions,
                        },
                    },
                    enabled: entry.enabled,
                    trust: entry.trust,
                    grant: entry.grant,
                    activate_on_start: entry.activate_on_start,
                }
            })
            .collect();
        config.app_api = self
            .app_api
            .into_iter()
            .map(|entry| AppApiRuntimeConfig {
                provider_id: entry.provider_id,
                base_url: entry.base_url,
                schema_digest: entry.schema_digest,
                credential_env: entry.credential_env,
                scope: entry.scope,
                capability: entry.capability,
                permission: entry.permission,
                allowed_mutations: entry.allowed_mutations,
                requests_per_minute: entry.requests_per_minute,
                timeout: entry.timeout,
                max_response_bytes: entry.max_response_bytes,
                enabled: entry.enabled,
                trust: entry.trust,
                grant: entry.grant,
            })
            .collect();
        config.validate()
    }
}

fn skill_definition(entry: SkillConfigEntry) -> SkillDefinition {
    SkillDefinition {
        schema_version: p::SchemaVersion(1),
        metadata: SkillMetadata {
            schema_version: p::SchemaVersion(1),
            id: entry.id,
            summary: entry.summary,
            scope: entry.scope,
            version: entry.version,
            trust: entry.trust,
        },
        body: SkillBody(entry.body),
    }
}

fn bounded_text(value: &str, label: &str) -> p::Result<()> {
    if value.trim().is_empty() || value.len() > MAX_CONFIG_TEXT_BYTES {
        Err(p::Error(format!("{label} is empty or exceeds its bound")))
    } else {
        Ok(())
    }
}
