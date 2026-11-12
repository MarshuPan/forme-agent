//! Fixed declarative sources and their immutable configuration publication.
use super::*;
use std::collections::BTreeSet;

fn load(template: &p::ProductConfigTemplate) -> p::Result<p::ProductPluginSourceCatalog> {
    let path = template
        .core_config_path
        .with_extension("plugin-sources.json");
    storage::check(&path)?;
    if !path.try_exists().map_err(|_| invalid())? {
        return Ok(p::ProductPluginSourceCatalog {
            schema_version: p::SchemaVersion(1),
            sources: Vec::new(),
        });
    }
    let catalog: p::ProductPluginSourceCatalog = storage::json(&path, MAX_CONFIG_BYTES)?;
    catalog.validate()?;
    Ok(catalog)
}

pub(super) fn prepare(
    template: &p::ProductConfigTemplate,
    state: &mut ConfigState,
) -> p::Result<()> {
    let Some(managed) = &state.managed_capabilities else {
        return Ok(());
    };
    let enabled = managed
        .plugins
        .iter()
        .filter(|p| p.enabled)
        .collect::<Vec<_>>();
    let catalog = if enabled.is_empty() {
        None
    } else {
        Some(load(template)?)
    };
    let mut sources = Vec::new();
    for settings in enabled {
        let source = catalog
            .as_ref()
            .and_then(|c| {
                c.sources
                    .iter()
                    .find(|s| s.source_ref == settings.source_ref)
            })
            .ok_or_else(rejected_patch)?;
        source.validate_settings(settings)?;
        sources.push(source.clone());
    }
    if !sources.is_empty() || !state.plugin_sources.is_empty() {
        state.schema_version = p::SchemaVersion(state.schema_version.0.max(13));
    }
    state.plugin_sources = sources;
    validate_state(state)
}

pub(super) fn validate_state(state: &ConfigState) -> p::Result<()> {
    p::ProductPluginSourceCatalog {
        schema_version: p::SchemaVersion(1),
        sources: state.plugin_sources.clone(),
    }
    .validate()?;
    if state.schema_version.0 < 13 && !state.plugin_sources.is_empty() {
        return Err(invalid());
    }
    let enabled = state
        .managed_capabilities
        .as_ref()
        .map(|m| m.plugins.iter().filter(|p| p.enabled).collect::<Vec<_>>())
        .unwrap_or_default();
    if enabled.len() != state.plugin_sources.len() {
        return Err(invalid());
    }
    let mut ids = state
        .capabilities
        .as_ref()
        .map(|c| {
            c.skills
                .iter()
                .map(|s| s.id.clone())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    for source in &state.plugin_sources {
        let settings = enabled
            .iter()
            .find(|p| p.source_ref == source.source_ref)
            .ok_or_else(invalid)?;
        source.validate_settings(settings)?;
        for skill in &source.skills {
            if !ids.insert(skill.id.clone()) {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

impl ProductConfigStore {
    pub fn plugin_sources(&self) -> p::Result<Vec<p::ProductPluginConfig>> {
        load(&self.template)?
            .sources
            .into_iter()
            .map(|source| {
                Ok(p::ProductPluginConfig {
                    schema_version: p::SchemaVersion(1),
                    expected_source_digest: source.digest()?,
                    source_ref: source.source_ref,
                    plugin_ref: source.plugin_ref,
                    scope: source.scope,
                    enabled: false,
                    trust: p::ProductSkillTrust::Untrusted,
                })
            })
            .collect()
    }
}
