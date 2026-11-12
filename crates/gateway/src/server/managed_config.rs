//! Explicit installed-service transport, revision and brain binding.
use super::*;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreServiceConfig {
    pub schema_version: p::SchemaVersion,
    pub install_ref: String,
    pub revision: u64,
    pub brain: forme_harness::ManagedBrainConfig,
    pub transport: ManagedGatewayTransport,
    pub drain_timeout_ms: u64,
    #[serde(default)]
    pub channel_ingress: Option<p::ProductChannelIngress>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedGatewayTransport {
    pub schema_version: p::SchemaVersion,
    pub bind: SocketAddr,
    pub owner_credential_path: PathBuf,
    pub web_credential_path: PathBuf,
    pub web_assertion_verifying_key: [u8; 32],
    pub web_session_fence_path: PathBuf,
    #[serde(default)]
    pub configuration_ack_path: Option<PathBuf>,
    #[serde(default)]
    pub work_admission_path: Option<PathBuf>,
    #[serde(default)]
    pub release_binding: Option<p::ProductReleaseBinding>,
    #[serde(default)]
    pub release_admission_path: Option<PathBuf>,
    #[serde(default)]
    pub release_readiness_path: Option<PathBuf>,
    pub max_body_bytes: usize,
    pub request_timeout_ms: u64,
    pub max_requests_per_minute: u32,
}

impl CoreServiceConfig {
    pub fn load(path: &Path) -> p::Result<Self> {
        let bytes = read_file(path, 256 * 1024)?;
        let config: Self = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        config.validate()?;
        if config
            .channel_ingress
            .as_ref()
            .is_some_and(|c| c.configuration_path != path)
        {
            return Err(invalid());
        }
        Ok(config)
    }

    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
            || self.revision == 0
            || self.transport.schema_version != p::SchemaVersion(1)
            || !self.transport.bind.ip().is_loopback()
            || self.transport.bind.port() == 0
            || self.transport.max_body_bytes == 0
            || self.transport.max_body_bytes > 4 * 1024 * 1024
            || self.transport.request_timeout_ms == 0
            || self.transport.request_timeout_ms > 600_000
            || self.transport.max_requests_per_minute == 0
        {
            return Err(invalid());
        }
        p::validate_product_id(&self.install_ref)?;
        self.brain.runtime_config()?;
        super::channels::validate_configuration(self)?;
        GatewayLifecycleConfig {
            schema_version: p::SchemaVersion(1),
            drain_timeout: Duration::from_millis(self.drain_timeout_ms),
        }
        .validate()?;
        let mut paths = vec![
            &self.transport.owner_credential_path,
            &self.transport.web_credential_path,
            &self.transport.web_session_fence_path,
        ];
        if let Some(path) = self.transport.configuration_ack_path.as_ref() {
            paths.push(path);
        }
        if let Some(path) = self.transport.work_admission_path.as_ref() {
            paths.push(path);
        }
        super::release_admission::validate_transport(self)?;
        paths.extend(self.transport.release_admission_path.iter());
        paths.extend(self.transport.release_readiness_path.iter());
        if self.brain.expected_restoration_fence.is_some()
            && self.transport.work_admission_path.is_none()
        {
            return Err(invalid());
        }
        for (index, path) in paths.iter().enumerate() {
            crate::web::config::check_path(path)?;
            if paths.iter().skip(index + 1).any(|other| path == other)
                || *path == &self.brain.store_path
                || path.starts_with(&self.brain.artifact_root)
            {
                return Err(invalid());
            }
        }
        VerifyingKey::from_bytes(&self.transport.web_assertion_verifying_key)
            .map_err(|_| invalid())?;
        Ok(())
    }

    pub(super) fn transport(&self) -> p::Result<GatewayServerConfig> {
        self.validate()?;
        let config = GatewayServerConfig {
            schema_version: p::SchemaVersion(1),
            bind: self.transport.bind.ip(),
            port: self.transport.bind.port(),
            owner: p::VerifiedPrincipal(self.brain.owner.0.clone()),
            agent_profile: self.brain.agent_profile.clone(),
            max_body_bytes: self.transport.max_body_bytes,
            request_timeout: Duration::from_millis(self.transport.request_timeout_ms),
            max_requests_per_minute: self.transport.max_requests_per_minute,
            scheduler_enabled: self.brain.scheduler_enabled,
            scheduler: self.brain.scheduler.clone(),
            install_ref: Some(self.install_ref.clone()),
            web_bridge_token: Some(credential(&self.transport.web_credential_path)?),
            web_assertion_verifying_key: Some(
                VerifyingKey::from_bytes(&self.transport.web_assertion_verifying_key)
                    .map_err(|_| invalid())?,
            ),
            web_session_fence_path: Some(self.transport.web_session_fence_path.clone()),
            token: credential(&self.transport.owner_credential_path)?,
            token_path: Some(self.transport.owner_credential_path.clone()),
            channel_ingress: self.channel_ingress.clone(),
        };
        config.validate()?;
        Ok(config)
    }

    pub(super) fn configuration_ack(&self) -> Option<p::ProductConfigAcknowledgement> {
        self.transport
            .configuration_ack_path
            .as_ref()
            .map(|_| p::ProductConfigAcknowledgement {
                schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
                install_ref: self.install_ref.clone(),
                revision: self.revision,
                data_identity: None,
            })
    }
}

fn credential(path: &Path) -> p::Result<LocalBearerSecret> {
    let value = String::from_utf8(read_file(path, 16 * 1024)?).map_err(|_| invalid())?;
    LocalBearerSecret::new(value.trim_end_matches(['\r', '\n']).to_owned()).map_err(|_| invalid())
}

pub(super) fn read_file(path: &Path, limit: u64) -> p::Result<Vec<u8>> {
    crate::web::config::check_path(path)?;
    let file = std::fs::File::open(path).map_err(|_| invalid())?;
    if !file.metadata().map_err(|_| invalid())?.is_file() {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 > limit {
        return Err(invalid());
    }
    Ok(bytes)
}

fn invalid() -> p::Error {
    p::Error("managed core service configuration is unavailable or invalid".into())
}

pub(super) fn write_configuration_ack(
    path: &Path,
    acknowledgement: &p::ProductConfigAcknowledgement,
) -> p::Result<()> {
    acknowledgement.validate()?;
    write_product_fact(path, acknowledgement)
}

pub(super) fn write_product_fact(path: &Path, fact: &impl serde::Serialize) -> p::Result<()> {
    crate::web::config::check_path(path)?;
    let parent = path.parent().ok_or_else(invalid)?;
    std::fs::create_dir_all(parent).map_err(|_| invalid())?;
    let bytes = serde_json::to_vec_pretty(fact).map_err(|_| invalid())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| invalid())?;
    use std::io::Write;
    file.write_all(&bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| invalid())?;
    crate::web::config::check_path(path)?;
    file.persist(path).map_err(|_| invalid())?;
    #[cfg(unix)]
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| invalid())?;
    Ok(())
}

#[cfg(test)]
pub(super) mod tests;
