//! V3 additions share the configuration journal and never start a capability.
use super::*;
use std::collections::BTreeMap;

pub(super) struct ManagedUpdate {
    pub(super) digest: p::SchemaDigest,
    pub(super) capabilities: Option<p::ProductManagedCapabilityPatch>,
    pub(super) channels: Option<p::ProductChannelSettings>,
}
impl ManagedUpdate {
    pub(super) fn validate(
        &self,
        desktop: Option<&super::super::InstalledDesktopProvisioning>,
    ) -> p::Result<()> {
        if let Some(value) = &self.capabilities {
            value.validate().map_err(|_| rejected_patch())?;
            for key in &value.publisher_keys {
                validate_key(&key.public_key).map_err(|_| rejected_patch())?;
            }
            // Enabling requires the Host's independently provisioned consumer.
            // Declarative disabled entries can be staged before that binding exists.
            if value.mcp_servers.iter().any(|v| v.enabled)
                || value.plugins.iter().any(|v| v.enabled)
            {
                return Err(rejected_patch());
            }
        }
        if let Some(value) = &self.capabilities {
            if value.computer.enabled {
                desktop
                    .ok_or_else(rejected_patch)?
                    .validate_settings(&value.computer)
                    .map_err(|_| rejected_patch())?;
            }
        }
        if let Some(value) = &self.channels {
            value.validate().map_err(|_| rejected_patch())?;
            if value.channels.iter().any(|v| v.enabled) {
                return Err(rejected_patch());
            }
        }
        Ok(())
    }
    pub(super) fn prepare(
        &self,
        template: &p::ProductConfigTemplate,
        id: &str,
        state: &mut ConfigState,
    ) -> p::Result<()> {
        if let Some(value) = &self.capabilities {
            state.managed_capabilities = Some(value.view());
            state.publisher_key_files = value
                .publisher_keys
                .iter()
                .map(|key| {
                    let digest = p::canonical_digest(&(
                        "forme.product.publisher.material.v1",
                        &key.publisher_ref,
                        &key.public_key_digest,
                    ))?;
                    Ok((
                        key.publisher_ref.clone(),
                        template
                            .model_credentials_directory
                            .join(id)
                            .join(format!("publisher-{}.key", &digest.0[7..])),
                    ))
                })
                .collect::<p::Result<BTreeMap<_, _>>>()?;
        }
        if let Some(value) = &self.channels {
            state.channels = Some(value.clone());
        }
        if state.managed_capabilities.is_some() || state.channels.is_some() {
            state.schema_version = p::SchemaVersion(state.schema_version.0.max(10));
        }
        Ok(())
    }
    pub(super) fn write_keys(&self, state: &ConfigState) -> p::Result<()> {
        if let Some(value) = &self.capabilities {
            for key in &value.publisher_keys {
                service_files::immutable(
                    state
                        .publisher_key_files
                        .get(&key.publisher_ref)
                        .ok_or_else(invalid)?,
                    &key.public_key,
                )?;
            }
        }
        Ok(())
    }
}
fn validate_key(bytes: &[u8; 32]) -> p::Result<()> {
    let key = ed25519_dalek::VerifyingKey::from_bytes(bytes).map_err(|_| invalid())?;
    if key.is_weak() {
        return Err(invalid());
    }
    Ok(())
}
pub(super) fn validate_state(
    state: &ConfigState,
    template: &p::ProductConfigTemplate,
) -> p::Result<()> {
    let added = state.managed_capabilities.is_some() || state.channels.is_some();
    if (added && state.schema_version.0 < 10) || (state.schema_version.0 == 10 && !added) {
        return Err(invalid());
    }
    let keys = state
        .managed_capabilities
        .as_ref()
        .map(|v| v.publisher_keys.as_slice())
        .unwrap_or_default();
    if keys.len() != state.publisher_key_files.len() {
        return Err(invalid());
    }
    if let Some(value) = &state.managed_capabilities {
        value.validate()?;
    }
    if let Some(value) = &state.channels {
        value.validate()?;
    }
    for key in keys {
        let path = state
            .publisher_key_files
            .get(&key.publisher_ref)
            .ok_or_else(invalid)?;
        storage::check(path)?;
        if !key.configured
            || !path.starts_with(&template.model_credentials_directory)
            || state
                .credentials
                .values()
                .any(|credential| credential == path)
        {
            return Err(invalid());
        }
    }
    Ok(())
}
pub(super) fn verify_keys(state: &ConfigState) -> p::Result<()> {
    if let Some(value) = &state.managed_capabilities {
        for key in &value.publisher_keys {
            let bytes = storage::read(
                state
                    .publisher_key_files
                    .get(&key.publisher_ref)
                    .ok_or_else(invalid)?,
                32,
            )?;
            let bytes: [u8; 32] = bytes.try_into().map_err(|_| invalid())?;
            validate_key(&bytes)?;
            if p::sha256_content_digest(&bytes) != key.public_key_digest {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

impl ProductConfigStore {
    pub fn snapshot_settings_v3(&mut self) -> p::Result<p::ProductConfigViewV3> {
        let view = self.snapshot_settings_v2()?;
        Ok(view.into_settings_v3(
            self.state.managed_capabilities.clone().unwrap_or_default(),
            self.state.channels.clone().unwrap_or_default(),
        ))
    }
    pub fn apply_settings_v3(
        &mut self,
        patch: p::ProductConfigPatchV3,
    ) -> p::Result<p::ProductConfigResponseV3> {
        patch.validate().map_err(|_| rejected_patch())?;
        let update = ManagedUpdate {
            digest: patch.digest()?,
            capabilities: patch.managed_capabilities,
            channels: patch.channels,
        };
        update.validate(
            self.state
                .desktop
                .as_ref()
                .or(self.installed_desktop.as_ref()),
        )?;
        let network = patch
            .network
            .as_ref()
            .map(|v| self.materialize_settings_network(v))
            .transpose()?;
        let result = self.apply_transaction(
            p::ProductConfigPatch {
                schema_version: p::PRODUCT_SETTINGS_SCHEMA_VERSION,
                operation_id: patch.operation_id,
                expected_revision: patch.expected_revision,
                owner: patch.owner,
                models: patch.models,
                brain: patch.brain,
                capabilities: patch.capabilities,
                network,
            },
            patch.proxy,
            Some(update),
        )?;
        Ok(p::ProductConfigResponseV3 {
            schema_version: p::PRODUCT_MANAGED_SETTINGS_SCHEMA_VERSION,
            view: self.snapshot_settings_v3()?,
            applied: result.applied,
            operation_id: result.operation_id,
            core_activation: None,
        })
    }
}
