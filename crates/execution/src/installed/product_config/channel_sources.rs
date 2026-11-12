//! Host validates local connector custody before publishing an enabled ingress.
use super::*;
fn load(template: &p::ProductConfigTemplate) -> p::Result<p::ProductChannelSourceCatalog> {
    let path = template
        .core_config_path
        .with_extension("channel-sources.json");
    storage::check(&path)?;
    if !path.try_exists().map_err(|_| invalid())? {
        return Ok(p::ProductChannelSourceCatalog {
            schema_version: p::SchemaVersion(1),
            sources: Vec::new(),
        });
    }
    let catalog: p::ProductChannelSourceCatalog = storage::json(&path, 128 * 1024)?;
    catalog.validate()?;
    // Connector identities never reuse the Core's owner/Web administration keys.
    for path in [
        &template.core_owner_credential_path,
        &template.core_web_credential_path,
    ] {
        let secret = storage::secret(path)?;
        let secret = std::str::from_utf8(&secret)
            .map_err(|_| invalid())?
            .trim_end_matches(['\r', '\n']);
        if catalog
            .sources
            .iter()
            .any(|s| s.credential_digest == p::sha256_content_digest(secret.as_bytes()))
        {
            return Err(invalid());
        }
    }
    Ok(catalog)
}
pub(super) fn prepare(
    template: &p::ProductConfigTemplate,
    state: &mut ConfigState,
    consumer: Option<&super::super::InstalledChannelDelivery>,
) -> p::Result<()> {
    let enabled = state
        .channels
        .as_ref()
        .map(|v| v.channels.iter().filter(|c| c.enabled).collect::<Vec<_>>())
        .unwrap_or_default();
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
                    .find(|s| s.channel_ref == settings.channel_ref)
            })
            .ok_or_else(rejected_patch)?;
        source.validate_settings(
            settings,
            state.owner.as_ref().ok_or_else(invalid)?,
            &workspace(template, state),
        )?;
        if settings.adapter_ref == p::ProductChannelAdapter::Loopback {
            consumer.ok_or_else(rejected_patch)?.available(source)?;
            if state.worker.is_none() {
                return Err(rejected_patch());
            }
        }
        sources.push(source.clone());
    }
    if !sources.is_empty() || !state.channel_sources.is_empty() {
        state.schema_version = p::SchemaVersion(state.schema_version.0.max(14));
    }
    state.channel_sources = sources;
    validate_state(state, template)
}
fn workspace(template: &p::ProductConfigTemplate, state: &ConfigState) -> p::WorkspaceRef {
    state
        .brain
        .as_ref()
        .map(|b| b.default_workspace.clone())
        .unwrap_or_else(|| template.workspace.clone())
}
pub(super) fn validate_state(
    state: &ConfigState,
    template: &p::ProductConfigTemplate,
) -> p::Result<()> {
    if state.schema_version.0 < 14 && !state.channel_sources.is_empty() {
        return Err(invalid());
    }
    let enabled = state
        .channels
        .as_ref()
        .map(|v| v.channels.iter().filter(|c| c.enabled).collect::<Vec<_>>())
        .unwrap_or_default();
    if enabled.len() != state.channel_sources.len() {
        return Err(invalid());
    }
    p::ProductChannelSourceCatalog {
        schema_version: p::SchemaVersion(1),
        sources: state.channel_sources.clone(),
    }
    .validate()?;
    for source in &state.channel_sources {
        let setting = enabled
            .iter()
            .find(|s| s.channel_ref == source.channel_ref)
            .ok_or_else(invalid)?;
        source.validate_settings(
            setting,
            state.owner.as_ref().ok_or_else(invalid)?,
            &workspace(template, state),
        )?;
    }
    Ok(())
}

impl ProductConfigStore {
    pub fn channel_sources(&self) -> p::Result<Vec<p::ProductChannelConfig>> {
        Ok(load(&self.template)?
            .sources
            .into_iter()
            .map(|s| p::ProductChannelConfig {
                schema_version: p::SchemaVersion(1),
                channel_ref: s.channel_ref,
                adapter_ref: if s.delivery.is_some() {
                    p::ProductChannelAdapter::Loopback
                } else {
                    p::ProductChannelAdapter::Text
                },
                connector_ref: s.connector_ref,
                scope: s.scope,
                enabled: false,
                participant_binding: s.participant_binding,
                credential_ref: Some(s.credential_ref),
            })
            .collect())
    }
}

impl ProductConfigStore {
    pub(crate) fn bind_channel_consumer(
        &mut self,
        consumer: &std::sync::Arc<super::super::InstalledChannelDelivery>,
    ) {
        self.installed_channel = std::sync::Arc::downgrade(consumer);
    }
    pub(crate) fn channel_delivery_source(
        &mut self,
        request: &p::WorkerDispatchRequest,
    ) -> p::Result<(
        p::ProductChannelSource,
        super::super::management_repair::storage::ReadCustody,
    )> {
        self.recover()?;
        self.verify_published()?;
        let p::ActionParameters::AppApi(spec) = &request
            .execution
            .as_ref()
            .ok_or_else(invalid)?
            .intent
            .parameters
        else {
            return Err(invalid());
        };
        if self.state.owner.as_ref().map(|v| &v.0) != Some(&request.job.owner_ref.0) {
            return Err(invalid());
        }
        let source = self
            .state
            .channel_sources
            .iter()
            .find(|s| {
                s.connector_ref == spec.connector.0
                    && Some(&s.participant) == spec.participant.as_ref()
            })
            .ok_or_else(invalid)?
            .clone();
        let settings = self
            .state
            .channels
            .as_ref()
            .ok_or_else(invalid)?
            .channels
            .iter()
            .find(|c| {
                c.channel_ref == source.channel_ref
                    && c.adapter_ref == p::ProductChannelAdapter::Loopback
            })
            .ok_or_else(invalid)?;
        source.validate_settings(
            settings,
            self.state.owner.as_ref().ok_or_else(invalid)?,
            &workspace(&self.template, &self.state),
        )?;
        let mut custody = super::super::management_repair::storage::ReadCustody::default();
        let catalog: p::ProductChannelSourceCatalog = custody.json(
            &self
                .template
                .core_config_path
                .with_extension("channel-sources.json"),
            128 * 1024,
        )?;
        catalog.validate()?;
        if !catalog.sources.contains(&source) {
            return Err(invalid());
        }
        Ok((source, custody))
    }
}
