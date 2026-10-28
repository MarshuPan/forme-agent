//! Managed Web-process configuration. No brain/model configuration is loaded.
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};

use ed25519_dalek::VerifyingKey;
use forme_protocol as p;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebServerConfig {
    pub schema_version: p::SchemaVersion,
    pub install_ref: String,
    pub identity_directory: PathBuf,
    pub claim_verifying_key: [u8; 32],
    #[serde(default = "default_bind")]
    pub bind: SocketAddr,
    /// Fixed installer-owned projection. Browser input never changes this
    /// path; the product configuration writer publishes the typed overlay.
    #[serde(default)]
    pub network_config_path: Option<PathBuf>,
    #[serde(default)]
    pub network_material_directory: Option<PathBuf>,
    #[serde(default)]
    pub tls: Option<super::WebTlsConfig>,
    #[serde(default)]
    pub bootstrap_bridge: Option<WebBootstrapBridgeConfig>,
    #[serde(default)]
    pub configuration_bridge: Option<WebConfigurationBridgeConfig>,
    #[serde(default)]
    pub core_bridge: Option<WebCoreBridgeConfig>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebBootstrapBridgeConfig {
    pub schema_version: p::SchemaVersion,
    pub address: SocketAddr,
    pub credential_path: PathBuf,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebConfigurationBridgeConfig {
    pub schema_version: p::SchemaVersion,
    pub address: SocketAddr,
    pub credential_path: PathBuf,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebCoreBridgeConfig {
    pub schema_version: p::SchemaVersion,
    pub address: SocketAddr,
    pub credential_path: PathBuf,
    pub signing_key_path: PathBuf,
}

fn default_bind() -> SocketAddr {
    p::DEFAULT_WEB_BIND.parse().expect("valid default Web bind")
}

impl WebServerConfig {
    /// The installer must protect this file and directory with the Web service
    /// ACL. Structural path checks do not certify native permissions.
    pub fn load(path: &Path) -> p::Result<Self> {
        check_path(path)?;
        let file = std::fs::File::open(path).map_err(|_| invalid())?;
        if !file.metadata().map_err(|_| invalid())?.is_file() {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        file.take(32 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        if bytes.len() > 32 * 1024 {
            return Err(invalid());
        }
        let mut config: Self = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        config.validate()?;
        config.apply_network_overlay()?;
        config.validate()?;
        Ok(config)
    }

    fn apply_network_overlay(&mut self) -> p::Result<()> {
        let Some(path) = self.network_config_path.clone() else {
            return Ok(());
        };
        check_path(&path)?;
        let file = std::fs::File::open(&path).map_err(|_| invalid())?;
        if !file.metadata().map_err(|_| invalid())?.is_file() {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        file.take(32 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        if bytes.len() > 32 * 1024 {
            return Err(invalid());
        }
        let network: p::ProductNetworkConfig =
            serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        network.validate_material_root(self.network_material_directory.as_deref())?;
        let p::ProductNetworkConfig {
            bind,
            allowed_hosts,
            tls,
            ..
        } = network;
        self.bind = bind;
        self.tls = tls.map(|tls| super::WebTlsConfig {
            schema_version: tls.schema_version,
            certificate_path: tls.certificate_path,
            private_key_path: tls.private_key_path,
            allowed_hosts,
        });
        Ok(())
    }

    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1) || self.bind.port() == 0 {
            return Err(invalid());
        }
        if let Some(tls) = &self.tls {
            tls.validate(self.bind)?;
        } else if !self.bind.ip().is_loopback() {
            return Err(p::Error(
                "plaintext Web ingress must remain loopback-only".into(),
            ));
        }
        p::validate_product_id(&self.install_ref)?;
        if self.network_config_path.is_some() != self.network_material_directory.is_some() {
            return Err(invalid());
        }
        if let Some(path) = &self.network_config_path {
            check_path(path)?;
        }
        if let Some(path) = &self.network_material_directory {
            check_path(path)?;
        }
        if let Some(bridge) = &self.bootstrap_bridge {
            if bridge.schema_version != p::SchemaVersion(1)
                || !bridge.address.ip().is_loopback()
                || bridge.address.port() == 0
                || bridge.address == self.bind
            {
                return Err(invalid());
            }
            check_path(&bridge.credential_path)?;
        }
        if let Some(bridge) = &self.core_bridge {
            if bridge.schema_version != p::SchemaVersion(1)
                || !bridge.address.ip().is_loopback()
                || bridge.address.port() == 0
                || bridge.address == self.bind
            {
                return Err(invalid());
            }
            check_path(&bridge.credential_path)?;
            check_path(&bridge.signing_key_path)?;
            if bridge.credential_path == bridge.signing_key_path {
                return Err(invalid());
            }
        }
        if let Some(bridge) = &self.configuration_bridge {
            if bridge.schema_version != p::SchemaVersion(1)
                || !bridge.address.ip().is_loopback()
                || bridge.address.port() == 0
                || bridge.address == self.bind
            {
                return Err(invalid());
            }
            check_path(&bridge.credential_path)?;
        }
        check_path(&self.identity_directory)?;
        VerifyingKey::from_bytes(&self.claim_verifying_key).map_err(|_| invalid())?;
        Ok(())
    }
}

fn invalid() -> p::Error {
    p::Error("Web service configuration is unavailable or invalid".into())
}

pub(crate) fn check_path(path: &Path) -> p::Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid());
    }
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(invalid());
                    }
                }
                if metadata.file_type().is_symlink() {
                    return Err(invalid());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(invalid()),
        }
    }
    Ok(())
}
