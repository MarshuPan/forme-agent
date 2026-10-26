//! Public settings versions share the existing writer and immutable revisions.
use super::*;

impl ProductConfigStore {
    pub fn snapshot(&mut self) -> p::Result<p::ProductConfigView> {
        self.recover()?;
        self.verify_published()?;
        self.sync_acknowledgement()?;
        let acknowledged = self.journal.acknowledged();
        let network = self
            .state
            .network
            .clone()
            .unwrap_or_else(|| p::ProductNetworkConfig::loopback(self.template.web_bind));
        let network_active = network == self.active_network()?;
        Ok(p::ProductConfigView {
            schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
            install_ref: self.template.install_ref.clone(),
            owner: self.state.owner.clone(),
            active_revision: self.state.revision,
            acknowledged_revision: acknowledged,
            pending_restart: (self.state.revision > 0 && acknowledged != Some(self.state.revision))
                || !network_active,
            configured: self.state.revision > 0,
            models: self.state.models.clone(),
            brain: self
                .state
                .brain
                .clone()
                .unwrap_or_else(|| p::ProductBrainPreferences {
                    schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
                    default_workspace: self.template.workspace.clone(),
                    model_routing: Default::default(),
                }),
            brain_source: if self.state.brain.is_some() {
                p::ProductPreferenceSource::ManagedConfiguration
            } else {
                p::ProductPreferenceSource::InstallationTemplate
            },
            issue_refs: Vec::new(),
            capabilities: self.state.capabilities.clone().unwrap_or_default(),
            capabilities_source: if self.state.capabilities.is_some() {
                p::ProductPreferenceSource::ManagedConfiguration
            } else {
                p::ProductPreferenceSource::InstallationTemplate
            },
            network,
            network_active,
            network_source: if self.state.network.is_some() {
                p::ProductPreferenceSource::ManagedConfiguration
            } else {
                p::ProductPreferenceSource::InstallationTemplate
            },
            availability: None,
        })
    }

    pub fn snapshot_with_schema(
        &mut self,
        version: p::SchemaVersion,
    ) -> p::Result<p::ProductConfigView> {
        p::validate_product_settings_schema(version)?;
        self.snapshot()?
            .with_settings_schema(version, self.network_is_managed())
    }

    pub fn snapshot_settings_v2(&mut self) -> p::Result<p::ProductConfigViewV2> {
        let view = self.snapshot()?;
        let network = self.settings_network_view(&view.network)?;
        let proxy = self
            .state
            .proxy
            .as_ref()
            .cloned()
            .unwrap_or_else(p::ProductProxyConfig::direct);
        let proxy_view = p::ProductProxySettingsView::from_internal(&proxy)?;
        let proxy_source = if self.state.proxy.is_some() {
            p::ProductPreferenceSource::ManagedConfiguration
        } else {
            p::ProductPreferenceSource::InstallationTemplate
        };
        Ok(view.into_settings_v2(
            network,
            proxy_view,
            proxy_source,
            p::ProductConfigAvailability::managed(self.network_is_managed()),
        ))
    }

    pub fn apply_settings_v2(
        &mut self,
        patch: p::ProductConfigPatchV2,
    ) -> p::Result<p::ProductConfigResponseV2> {
        patch.validate().map_err(|_| rejected_patch())?;
        let network = patch
            .network
            .as_ref()
            .map(|network| self.materialize_settings_network(network))
            .transpose()?;
        let proxy = patch.proxy;
        let response = self.apply_internal(
            p::ProductConfigPatch {
                schema_version: patch.schema_version,
                operation_id: patch.operation_id,
                expected_revision: patch.expected_revision,
                owner: patch.owner,
                models: patch.models,
                brain: patch.brain,
                capabilities: patch.capabilities,
                network,
            },
            proxy,
        )?;
        Ok(p::ProductConfigResponseV2 {
            schema_version: p::PRODUCT_SETTINGS_SCHEMA_VERSION,
            view: self.snapshot_settings_v2()?,
            applied: response.applied,
            operation_id: response.operation_id,
        })
    }

    pub(super) fn settings_response(
        &mut self,
        operation: String,
        applied: bool,
        version: p::SchemaVersion,
    ) -> p::Result<p::ProductConfigResponse> {
        self.response(operation, applied)?
            .with_settings_schema(version, self.network_is_managed())
    }

    fn settings_network_view(
        &self,
        network: &p::ProductNetworkConfig,
    ) -> p::Result<p::ProductNetworkSettingsView> {
        let material_configured =
            network.tls.is_none() || super::network::validation_digest(network).is_ok();
        p::ProductNetworkSettingsView::from_internal(network, material_configured)
    }

    fn materialize_settings_network(
        &self,
        settings: &p::ProductNetworkSettingsPatch,
    ) -> p::Result<p::ProductNetworkConfig> {
        settings.validate().map_err(|_| rejected_patch())?;
        let tls = settings.tls.as_ref().map(|_| {
            let root = self
                .template
                .web_material_directory
                .as_ref()
                .ok_or_else(rejected_patch)?;
            let existing = self
                .state
                .network
                .as_ref()
                .and_then(|network| network.tls.as_ref())
                .filter(|tls| {
                    tls.certificate_path.starts_with(root) && tls.private_key_path.starts_with(root)
                });
            Ok(existing.cloned().unwrap_or_else(|| p::ProductNetworkTls {
                schema_version: p::PRODUCT_NETWORK_SCHEMA_VERSION,
                certificate_path: root.join("server.pem"),
                private_key_path: root.join("server.key"),
            }))
        });
        let tls = tls.transpose()?;
        let network = p::ProductNetworkConfig {
            schema_version: p::PRODUCT_NETWORK_SCHEMA_VERSION,
            bind: settings.bind,
            allowed_hosts: settings.allowed_hosts.clone(),
            tls,
            firewall: settings.firewall.clone(),
        };
        network
            .validate_material_root(self.template.web_material_directory.as_deref())
            .map_err(|_| rejected_patch())?;
        Ok(network)
    }
}
