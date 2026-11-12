//! Protected declarative sources use the ordinary plugin/skill registries.
use super::*;
use forme_capabilities::{
    LocalPlugin, PluginContribution, PluginManifest, SkillBody, SkillDefinition, SkillMetadata,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub(crate) struct ManagedPluginSources {
    path: PathBuf,
    owner: p::OwnerPrincipalRef,
    sources: Vec<(p::ProductPluginSource, p::ProductPluginConfig)>,
}

impl ManagedBrainConfig {
    pub(crate) fn plugin_sources_guard(&self) -> p::Result<Option<ManagedPluginSources>> {
        p::ProductPluginSourceCatalog {
            schema_version: p::SchemaVersion(1),
            sources: self.plugin_sources.clone(),
        }
        .validate()?;
        let enabled = self
            .managed_capabilities
            .as_ref()
            .map(|m| m.plugins.iter().filter(|p| p.enabled).collect::<Vec<_>>())
            .unwrap_or_default();
        if enabled.len() != self.plugin_sources.len() {
            return Err(invalid());
        }
        if enabled.is_empty() {
            return Ok(None);
        }
        let path = self.plugin_configuration_path.clone().ok_or_else(invalid)?;
        check_managed_path(&path)?;
        if path == self.store_path
            || path.starts_with(&self.artifact_root)
            || path.starts_with(self.content_key_directory()?)
        {
            return Err(invalid());
        }
        let mut ids = self
            .capabilities
            .as_ref()
            .map(|c| {
                c.skills
                    .iter()
                    .map(|s| s.id.clone())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let mut sources = Vec::new();
        let mut seen = BTreeSet::new();
        for source in &self.plugin_sources {
            let settings = enabled
                .iter()
                .find(|p| p.source_ref == source.source_ref)
                .ok_or_else(invalid)?;
            source.validate_settings(settings)?;
            if !seen.insert(&source.plugin_ref) {
                return Err(invalid());
            }
            for skill in &source.skills {
                if !ids.insert(skill.id.clone()) {
                    return Err(invalid());
                }
            }
            sources.push((source.clone(), (*settings).clone()));
        }
        let guard = ManagedPluginSources {
            path,
            owner: self.owner.clone(),
            sources,
        };
        if guard.current_sources()?
            != guard
                .sources
                .iter()
                .map(|(s, _)| s.clone())
                .collect::<Vec<_>>()
        {
            return Err(invalid());
        }
        Ok(Some(guard))
    }

    pub(super) fn configure_plugins(
        &self,
        capabilities: &mut CapabilityRuntimeConfig,
    ) -> p::Result<()> {
        let Some(guard) = self.plugin_sources_guard()? else {
            return Ok(());
        };
        for (source, settings) in guard.sources {
            let mut contributions: Vec<PluginContribution> = source
                .skills
                .iter()
                .map(|skill| {
                    PluginContribution::Skill(SkillDefinition {
                        schema_version: p::SchemaVersion(1),
                        metadata: SkillMetadata {
                            schema_version: p::SchemaVersion(1),
                            id: skill.id.clone(),
                            summary: skill.summary.clone(),
                            scope: skill.scope.clone(),
                            version: skill.version,
                            trust: p::TrustTier::Untrusted,
                        },
                        body: SkillBody(skill.body.clone()),
                    })
                })
                .collect();
            for slot in &source.tool_slots {
                contributions.push(PluginContribution::Tool(
                    forme_capabilities::CapabilityDescriptor {
                        schema_version: p::SchemaVersion(1),
                        id: p::CapabilityRef(slot.id.0.clone()),
                        capability: forme_capabilities::Capability::Tool(slot.id.clone()),
                        scope: source.scope.clone(),
                        permissions: Vec::new(),
                        risk: Some(p::Risk::Low),
                        enabled: true,
                    },
                ));
                contributions.extend(
                    slot.hooks
                        .iter()
                        .map(|h| PluginContribution::Hook(plugin_hook(h))),
                );
            }
            capabilities.plugins.push(crate::PluginRuntimeConfig {
                plugin: LocalPlugin {
                    schema_version: p::SchemaVersion(1),
                    manifest: PluginManifest {
                        schema_version: p::SchemaVersion(1),
                        id: p::PluginRef(source.plugin_ref),
                        manifest_ref: p::PluginManifestRef(source.manifest_ref),
                        trust: p::TrustTier::Untrusted,
                        contributions,
                    },
                },
                enabled: true,
                trust: Some(p::TrustTier::OwnerInput),
                activate_on_start: true,
                grant: Some(p::AutonomyEnvelope {
                    schema_version: p::SchemaVersion(1),
                    scope: settings.scope,
                    capability: p::CapabilitySet {
                        schema_version: p::SchemaVersion(1),
                        capabilities: source
                            .skills
                            .iter()
                            .map(|s| p::CapabilityRef(s.id.0.clone()))
                            .chain(source.tool_slots.iter().flat_map(|s| {
                                std::iter::once(p::CapabilityRef(s.id.0.clone()))
                                    .chain(s.hooks.iter().map(|h| p::CapabilityRef(h.id.0.clone())))
                            }))
                            .collect(),
                        permissions: Vec::new(),
                    },
                    action_type: vec![p::ActionType::Observe],
                    risk_limit: p::Risk::Low,
                    approval_rule: p::ApprovalRule::Ask,
                    budget: capabilities.envelope_budget.clone(),
                    timebox: p::Timebox {
                        schema_version: p::SchemaVersion(1),
                        starts_at: 0,
                        expires_at: i64::MAX,
                        max_turns: 1,
                    },
                    rollback: p::RollbackReq {
                        schema_version: p::SchemaVersion(1),
                        required: false,
                        boundary: None,
                    },
                }),
            });
        }
        Ok(())
    }
}

impl ManagedPluginSources {
    pub(crate) fn revoked_skills(&self) -> p::Result<BTreeSet<p::SkillRef>> {
        let current = self.current_skills()?;
        Ok(self
            .sources
            .iter()
            .flat_map(|(source, _)| source.skills.iter())
            .filter(|skill| !current.contains(&skill.id))
            .map(|skill| skill.id.clone())
            .collect())
    }

    pub(crate) fn current_skills(&self) -> p::Result<BTreeSet<p::SkillRef>> {
        Ok(self
            .current_sources()?
            .iter()
            .flat_map(|s| s.skills.iter().map(|s| s.id.clone()))
            .collect())
    }
    pub(crate) fn settings(
        &self,
        source: &p::ProductPluginSource,
    ) -> p::Result<p::ProductPluginConfig> {
        self.sources
            .iter()
            .find(|(s, _)| s == source)
            .map(|(_, c)| c.clone())
            .ok_or_else(invalid)
    }
    pub(crate) fn require_source(&self, settings: &p::ProductPluginConfig) -> p::Result<()> {
        let source = self
            .current_sources()?
            .into_iter()
            .find(|s| s.plugin_ref == settings.plugin_ref)
            .ok_or_else(invalid)?;
        if self.settings(&source)? != *settings {
            return Err(invalid());
        }
        source.validate_settings(settings)
    }
    pub(crate) fn current_sources(&self) -> p::Result<Vec<p::ProductPluginSource>> {
        let bytes = read_managed_file(&self.path, 256 * 1024)?;
        let current: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        let owner: p::OwnerPrincipalRef =
            serde_json::from_value(current["brain"]["owner"].clone()).map_err(|_| invalid())?;
        if owner != self.owner {
            return Err(invalid());
        }
        let settings: p::ProductManagedCapabilitySettings =
            serde_json::from_value(current["brain"]["managed_capabilities"].clone())
                .map_err(|_| invalid())?;
        settings.validate()?;
        let current_sources: Vec<p::ProductPluginSource> =
            serde_json::from_value(current["brain"]["plugin_sources"].clone())
                .map_err(|_| invalid())?;
        let catalog: p::ProductPluginSourceCatalog = serde_json::from_slice(&read_managed_file(
            &self.path.with_extension("plugin-sources.json"),
            256 * 1024,
        )?)
        .map_err(|_| invalid())?;
        catalog.validate()?;
        let configured: BTreeMap<_, _> = settings
            .plugins
            .iter()
            .map(|p| (&p.plugin_ref, p))
            .collect();
        let mut sources = Vec::new();
        for (source, original) in &self.sources {
            if configured.get(&original.plugin_ref) != Some(&original)
                || !current_sources.contains(source)
                || !catalog.sources.contains(source)
            {
                continue;
            }
            sources.push(source.clone());
        }
        Ok(sources)
    }
}

fn plugin_hook(hook: &p::ProductPluginHookSlot) -> forme_capabilities::Hook {
    use forme_capabilities::{Hook, HookPoint, HookTransform};
    Hook {
        schema_version: p::SchemaVersion(1),
        id: hook.id.clone(),
        point: match hook.point {
            p::ProductPluginHookPoint::BeforePolicy => HookPoint::BeforePolicy,
            p::ProductPluginHookPoint::BeforeExecution => HookPoint::BeforeExecution,
        },
        transform: match &hook.transform {
            p::ProductPluginHookTransform::Identity => HookTransform::Identity,
            p::ProductPluginHookTransform::ReplaceCapability { capability } => {
                HookTransform::ReplaceCapability(capability.clone())
            }
            p::ProductPluginHookTransform::ReplaceShellProgram { program } => {
                HookTransform::ReplaceShellProgram(program.clone())
            }
        },
    }
}
