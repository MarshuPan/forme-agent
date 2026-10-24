//! Installed HTTPS material. The manager owns files; Web has read access only.
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use forme_protocol as p;
use rustls_pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebTlsConfig {
    pub schema_version: p::SchemaVersion,
    pub certificate_path: PathBuf,
    pub private_key_path: PathBuf,
    pub allowed_hosts: Vec<String>,
}

impl WebTlsConfig {
    pub fn validate(&self, bind: SocketAddr) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
            || bind.ip().is_unspecified()
            || bind.ip().is_multicast()
            || bind.port() == 0
            || self.certificate_path == self.private_key_path
            || self.allowed_hosts.is_empty()
            || self.allowed_hosts.len() > 16
        {
            return Err(invalid());
        }
        super::config::check_path(&self.certificate_path)?;
        super::config::check_path(&self.private_key_path)?;
        p::validate_product_network_hosts(&self.allowed_hosts, bind.port())
            .map_err(|_| invalid())?;
        Ok(())
    }

    /// This validates PEM and key/certificate binding, not browser trust or the
    /// owner's firewall. No certificate, key bytes or parser errors are logged.
    pub(crate) fn load(&self) -> p::Result<tokio_rustls::TlsAcceptor> {
        let certificates = read_material(&self.certificate_path)?;
        let certificates = CertificateDer::pem_slice_iter(&certificates)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| invalid())?;
        if certificates.is_empty() || certificates.len() > 8 {
            return Err(invalid());
        }
        let key = read_material(&self.private_key_path)?;
        let key = PrivateKeyDer::from_pem_slice(&key).map_err(|_| invalid())?;
        let mut server = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|_| invalid())?
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .map_err(|_| invalid())?;
        server.alpn_protocols = vec![b"http/1.1".to_vec()];
        server.max_early_data_size = 0;
        Ok(tokio_rustls::TlsAcceptor::from(Arc::new(server)))
    }
}

fn read_material(path: &Path) -> p::Result<Zeroizing<Vec<u8>>> {
    super::config::check_path(path)?;
    let file = std::fs::File::open(path).map_err(|_| invalid())?;
    if !file.metadata().map_err(|_| invalid())?.is_file() {
        return Err(invalid());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.is_empty() || bytes.len() > 65_536 {
        return Err(invalid());
    }
    Ok(bytes)
}

fn invalid() -> p::Error {
    p::Error("HTTPS configuration requires valid bounded certificate, key and exact hosts".into())
}
