//! Managed Web-process configuration. No brain/model configuration is loaded.
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
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
}

fn default_bind() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8765)
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
        let config: Self = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
            || !self.bind.ip().is_loopback()
            || self.bind.port() == 0
        {
            // This adapter has no TLS terminator yet. Fail closed for all LAN
            // addresses; no proxy header or JSON flag enables plaintext LAN.
            return Err(p::Error("Web configuration requires an exact loopback HTTP address; LAN TLS is not available in this adapter".into()));
        }
        p::validate_product_id(&self.install_ref)?;
        check_path(&self.identity_directory)?;
        VerifyingKey::from_bytes(&self.claim_verifying_key).map_err(|_| invalid())?;
        Ok(())
    }
}

fn invalid() -> p::Error {
    p::Error("Web service configuration is unavailable or invalid".into())
}

fn check_path(path: &Path) -> p::Result<()> {
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
