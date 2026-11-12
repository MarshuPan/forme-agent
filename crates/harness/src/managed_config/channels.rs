//! Channel sources are a Host projection; every recheck observes current settings.
use super::*;
#[derive(Clone)]
pub(crate) struct ManagedChannels {
    pub(crate) ingress: p::ProductChannelIngress,
    pub(crate) binding: ManagedWorkerBinding,
    owner: p::OwnerPrincipalRef,
    workspace: p::WorkspaceRef,
}
impl ManagedBrainConfig {
    pub(crate) fn channel_sources_guard(&self) -> p::Result<Option<ManagedChannels>> {
        let Some(ingress) = &self.channel_delivery else {
            return Ok(None);
        };
        if ingress.schema_version != p::SchemaVersion(1) || ingress.sources.is_empty() {
            return Err(invalid());
        }
        check_managed_path(&ingress.configuration_path)?;
        let mut binding = self.worker_binding.clone().ok_or_else(invalid)?;
        binding.validate()?;
        if !binding.supports(p::BackendKind::File) {
            return Err(invalid());
        }
        binding.backend_kinds = [p::BackendKind::AppApi].into_iter().collect();
        binding.sandbox_profile_ref = p::MANAGED_CHANNEL_PROFILE.into();
        let value = ManagedChannels {
            ingress: ingress.clone(),
            binding,
            owner: self.owner.clone(),
            workspace: self.workspace.clone(),
        };
        value.validate_sources()?;
        Ok(Some(value))
    }
}
impl ManagedChannels {
    fn validate_sources(&self) -> p::Result<()> {
        p::ProductChannelSourceCatalog {
            schema_version: p::SchemaVersion(1),
            sources: self.ingress.sources.clone(),
        }
        .validate()?;
        for s in &self.ingress.sources {
            if s.delivery.is_none()
                || s.scope.0 != self.workspace.0
                || s.participant.0 == self.owner.0
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub(crate) fn current(&self) -> p::Result<()> {
        self.validate_sources()?;
        let value: serde_json::Value =
            serde_json::from_slice(&read(&self.ingress.configuration_path)?)
                .map_err(|_| invalid())?;
        if value["brain"]["owner"].as_str() != Some(self.owner.0.as_str())
            || value["brain"]["workspace"].as_str() != Some(self.workspace.0.as_str())
            || serde_json::from_value::<p::ProductChannelIngress>(
                value["brain"]["channel_delivery"].clone(),
            )
            .map_err(|_| invalid())?
                != self.ingress
        {
            return Err(invalid());
        }
        let settings: p::ProductChannelSettings =
            serde_json::from_value(value["brain"]["channels"].clone()).map_err(|_| invalid())?;
        settings.validate()?;
        let catalog: p::ProductChannelSourceCatalog = serde_json::from_slice(&read(
            &self
                .ingress
                .configuration_path
                .with_extension("channel-sources.json"),
        )?)
        .map_err(|_| invalid())?;
        catalog.validate()?;
        for source in &self.ingress.sources {
            let setting = settings
                .channels
                .iter()
                .find(|v| v.channel_ref == source.channel_ref)
                .ok_or_else(invalid)?;
            source.validate_settings(setting, &self.owner, &self.workspace)?;
            if !catalog.sources.contains(source) {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub(crate) fn recheck(&self, intent: &p::ActionIntent) -> p::Result<()> {
        self.current()?;
        let p::ActionParameters::AppApi(spec) = &intent.parameters else {
            return Err(invalid());
        };
        let source = self
            .ingress
            .sources
            .iter()
            .find(|s| {
                s.connector_ref == spec.connector.0
                    && Some(&s.participant) == spec.participant.as_ref()
            })
            .ok_or_else(invalid)?;
        let delivery = source.delivery.as_ref().ok_or_else(invalid)?;
        if intent.source != p::Source::Communication
            || intent.action_type != p::ActionType::Deliver
            || intent.scope != source.scope
            || intent.capability_ref != delivery.capability_ref
            || intent.requested_permissions != [delivery.permission.clone()]
            || spec.endpoint != delivery.endpoint
            || spec.schema_digest != delivery.schema_digest
            || spec.credential.as_ref().map(|v| &v.0) != Some(&delivery.credential_ref)
        {
            return Err(invalid());
        }
        Ok(())
    }
}
fn read(path: &Path) -> p::Result<Vec<u8>> {
    check_managed_path(path)?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| invalid())?
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() > 256 * 1024 {
        return Err(invalid());
    }
    Ok(bytes)
}
