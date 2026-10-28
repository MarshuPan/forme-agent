//! Typed Web exposure settings owned by the installed-product configuration.
use super::*;
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::path::{Component, Path, PathBuf};

pub const PRODUCT_NETWORK_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);
pub const PRODUCT_NETWORK_MATERIAL_REF: &str = "managed:web";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductNetworkConfig {
    pub schema_version: SchemaVersion,
    pub bind: SocketAddr,
    pub allowed_hosts: Vec<String>,
    pub tls: Option<ProductNetworkTls>,
    pub firewall: Option<ProductFirewallBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductNetworkTls {
    pub schema_version: SchemaVersion,
    pub certificate_path: PathBuf,
    pub private_key_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductFirewallBinding {
    pub schema_version: SchemaVersion,
    pub rule_ref: String,
}

/// Public settings v2 never carries a certificate or private-key path. The
/// host resolves the fixed managed material reference inside its own store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductNetworkSettingsPatch {
    pub schema_version: SchemaVersion,
    pub bind: SocketAddr,
    pub allowed_hosts: Vec<String>,
    pub tls: Option<ProductNetworkTlsSettingsPatch>,
    pub firewall: Option<ProductFirewallBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductNetworkTlsSettingsPatch {
    pub schema_version: SchemaVersion,
    pub material_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductNetworkSettingsView {
    pub schema_version: SchemaVersion,
    pub bind: SocketAddr,
    pub allowed_hosts: Vec<String>,
    pub tls: Option<ProductNetworkTlsSettingsView>,
    pub firewall: Option<ProductFirewallBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductNetworkTlsSettingsView {
    pub schema_version: SchemaVersion,
    pub material_ref: String,
    pub material_configured: bool,
}

impl ProductNetworkSettingsPatch {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_NETWORK_SCHEMA_VERSION {
            return Err(Error("product network settings are invalid".into()));
        }
        validate_network_shape(
            self.bind,
            &self.allowed_hosts,
            self.tls.is_some(),
            self.firewall.as_ref(),
        )?;
        if let Some(tls) = &self.tls {
            if tls.schema_version != PRODUCT_NETWORK_SCHEMA_VERSION
                || tls.material_ref != PRODUCT_NETWORK_MATERIAL_REF
            {
                return Err(Error(
                    "product network TLS material reference is invalid".into(),
                ));
            }
        }
        Ok(())
    }
}

impl ProductNetworkSettingsView {
    pub fn from_internal(
        network: &ProductNetworkConfig,
        material_configured: bool,
    ) -> Result<Self> {
        network.validate()?;
        Ok(Self {
            schema_version: PRODUCT_NETWORK_SCHEMA_VERSION,
            bind: network.bind,
            allowed_hosts: network.allowed_hosts.clone(),
            tls: network.tls.as_ref().map(|_| ProductNetworkTlsSettingsView {
                schema_version: PRODUCT_NETWORK_SCHEMA_VERSION,
                material_ref: PRODUCT_NETWORK_MATERIAL_REF.into(),
                material_configured,
            }),
            firewall: network.firewall.clone(),
        })
    }
}

impl ProductNetworkConfig {
    pub fn loopback(bind: SocketAddr) -> Self {
        Self {
            schema_version: PRODUCT_NETWORK_SCHEMA_VERSION,
            bind,
            allowed_hosts: Vec::new(),
            tls: None,
            firewall: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_NETWORK_SCHEMA_VERSION {
            return Err(Error("product network configuration is invalid".into()));
        }
        validate_network_shape(
            self.bind,
            &self.allowed_hosts,
            self.tls.is_some(),
            self.firewall.as_ref(),
        )?;
        if let Some(tls) = &self.tls {
            if tls.schema_version != PRODUCT_NETWORK_SCHEMA_VERSION
                || tls.certificate_path == tls.private_key_path
                || self.allowed_hosts.is_empty()
                || !safe_absolute_path(&tls.certificate_path)
                || !safe_absolute_path(&tls.private_key_path)
            {
                return Err(Error("product network TLS configuration is invalid".into()));
            }
        } else if !self.allowed_hosts.is_empty() {
            return Err(Error(
                "product network hosts require an HTTPS configuration".into(),
            ));
        }
        if let Some(firewall) = &self.firewall {
            if firewall.schema_version != PRODUCT_NETWORK_SCHEMA_VERSION {
                return Err(Error("product firewall binding is invalid".into()));
            }
            validate_product_id(&firewall.rule_ref)?;
        }
        if self.bind.ip().is_loopback() {
            if self.firewall.is_some() {
                return Err(Error(
                    "loopback Web configuration must not require a firewall rule".into(),
                ));
            }
        } else if self.tls.is_none() || self.allowed_hosts.is_empty() || self.firewall.is_none() {
            return Err(Error(
                "LAN Web configuration requires HTTPS, hosts and a firewall binding".into(),
            ));
        }
        Ok(())
    }

    pub fn validate_material_root(&self, root: Option<&Path>) -> Result<()> {
        self.validate()?;
        let Some(tls) = &self.tls else {
            return Ok(());
        };
        let root = root.ok_or_else(|| Error("network TLS material root is unavailable".into()))?;
        if !safe_absolute_path(root)
            || !tls.certificate_path.starts_with(root)
            || !tls.private_key_path.starts_with(root)
        {
            return Err(Error(
                "network TLS material must remain inside the managed Web directory".into(),
            ));
        }
        Ok(())
    }

    /// The host binds this digest to the configuration revision before a Web
    /// restart. It is not an authorization by itself; the host still owns the
    /// firewall and service transition.
    pub fn validation_digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        crate::canonical_digest(&("forme-product-network-validation-v1", self))
    }
}

fn validate_network_shape(
    bind: SocketAddr,
    allowed_hosts: &[String],
    tls_configured: bool,
    firewall: Option<&ProductFirewallBinding>,
) -> Result<()> {
    if bind.port() == 0 || bind.ip().is_unspecified() || bind.ip().is_multicast() {
        return Err(Error("product network configuration is invalid".into()));
    }
    validate_product_network_hosts(allowed_hosts, bind.port())?;
    if !tls_configured && !allowed_hosts.is_empty() {
        return Err(Error(
            "product network hosts require an HTTPS configuration".into(),
        ));
    }
    if let Some(firewall) = firewall {
        if firewall.schema_version != PRODUCT_NETWORK_SCHEMA_VERSION {
            return Err(Error("product firewall binding is invalid".into()));
        }
        validate_product_id(&firewall.rule_ref)?;
    }
    if bind.ip().is_loopback() {
        if firewall.is_some() {
            return Err(Error(
                "loopback Web configuration must not require a firewall rule".into(),
            ));
        }
    } else if !tls_configured || allowed_hosts.is_empty() || firewall.is_none() {
        return Err(Error(
            "LAN Web configuration requires HTTPS, hosts and a firewall binding".into(),
        ));
    }
    Ok(())
}

/// Validate the exact Host authorities accepted by both the product config
/// projection and the standalone Web TLS loader.
pub fn validate_product_network_hosts(hosts: &[String], port: u16) -> Result<()> {
    if hosts.len() > 16 || port == 0 {
        return Err(Error("product network host allowlist is invalid".into()));
    }
    if hosts.is_empty() {
        return Ok(());
    }
    let mut seen_hosts = BTreeSet::new();
    for host in hosts {
        validate_authority(host, port)?;
        if !seen_hosts.insert(host) {
            return Err(Error("product network hosts must be unique".into()));
        }
    }
    Ok(())
}

fn safe_absolute_path(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
}

fn validate_authority(value: &str, port: u16) -> Result<()> {
    if value.is_empty()
        || value.len() > 260
        || !value.is_ascii()
        || value != value.to_ascii_lowercase()
        || value
            .chars()
            .any(|character| character.is_ascii_whitespace())
        || value.contains(['@', '%', '*', '/', '?', '#'])
    {
        return Err(Error("product network host authority is invalid".into()));
    }
    let (host, actual_port, bracketed) = if value.starts_with('[') {
        let end = value
            .find(']')
            .ok_or_else(|| Error("product network host authority is invalid".into()))?;
        if end <= 1 {
            return Err(Error("product network host authority is invalid".into()));
        }
        let port_text = value
            .get(end + 1..)
            .and_then(|suffix| suffix.strip_prefix(':'))
            .ok_or_else(|| Error("product network host authority is invalid".into()))?;
        (&value[1..end], port_text, true)
    } else {
        let (host, port_text) = value
            .rsplit_once(':')
            .ok_or_else(|| Error("product network host authority is invalid".into()))?;
        (host, port_text, false)
    };
    if host.is_empty() || actual_port.parse::<u16>().ok() != Some(port) {
        return Err(Error("product network host authority is invalid".into()));
    }
    if let Ok(address) = host.parse::<IpAddr>() {
        if address.is_unspecified() || address.is_multicast() || (address.is_ipv6() != bracketed) {
            return Err(Error("product network host authority is invalid".into()));
        }
        return Ok(());
    }
    if bracketed {
        return Err(Error("product network host authority is invalid".into()));
    }
    if host.ends_with('.') || host.len() > 253 {
        return Err(Error("product network host authority is invalid".into()));
    }
    if host.split('.').any(|part| {
        part.is_empty()
            || part.len() > 63
            || part.starts_with('-')
            || part.ends_with('-')
            || !part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }) {
        return Err(Error("product network host authority is invalid".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\forme\web-material")
        } else {
            PathBuf::from("/forme/web-material")
        }
    }

    fn lan() -> ProductNetworkConfig {
        let root = material_root();
        ProductNetworkConfig {
            schema_version: PRODUCT_NETWORK_SCHEMA_VERSION,
            bind: "192.168.1.20:8765".parse().unwrap(),
            allowed_hosts: vec!["192.168.1.20:8765".into()],
            tls: Some(ProductNetworkTls {
                schema_version: PRODUCT_NETWORK_SCHEMA_VERSION,
                certificate_path: root.join("cert.pem"),
                private_key_path: root.join("key.pem"),
            }),
            firewall: Some(ProductFirewallBinding {
                schema_version: PRODUCT_NETWORK_SCHEMA_VERSION,
                rule_ref: "firewall:forme-web-lan".into(),
            }),
        }
    }

    #[test]
    fn loopback_is_safe_by_default() {
        let network = ProductNetworkConfig::loopback(DEFAULT_WEB_BIND.parse().unwrap());
        assert!(network.validate().is_ok());
        assert!(network.validation_digest().is_ok());
    }

    #[test]
    fn lan_requires_tls_hosts_and_firewall() {
        let mut network = lan();
        assert!(network.validate().is_ok());
        network.tls = None;
        assert!(network.validate().is_err());
        let mut network = lan();
        network.firewall = None;
        assert!(network.validate().is_err());
        let mut network = lan();
        network.allowed_hosts.clear();
        assert!(network.validate().is_err());
    }

    #[test]
    fn material_must_stay_inside_managed_directory() {
        let mut network = lan();
        let root = material_root();
        assert!(network.validate_material_root(Some(&root)).is_ok());
        network.tls.as_mut().unwrap().private_key_path = if cfg!(windows) {
            PathBuf::from(r"C:\outside\key.pem")
        } else {
            PathBuf::from("/outside/key.pem")
        };
        assert!(network.validate_material_root(Some(&root)).is_err());
    }

    #[test]
    fn host_authorities_are_unique_and_ipv6_is_explicitly_bracketed() {
        let mut network = lan();
        network.allowed_hosts.push("192.168.1.20:8765".into());
        assert!(network.validate().is_err());

        for host in [
            "::1:8765",
            "[127.0.0.1]:8765",
            "[2001:db8::1]8765",
            "[2001:db8::1]:8764",
        ] {
            let mut network = lan();
            network.allowed_hosts = vec![host.into()];
            assert!(network.validate().is_err(), "{host}");
        }

        let mut network = lan();
        network.allowed_hosts = vec!["[2001:db8::1]:8765".into()];
        assert!(network.validate().is_ok());
    }

    #[test]
    fn public_network_settings_expose_managed_material_without_paths() {
        let view = ProductNetworkSettingsView::from_internal(&lan(), true).unwrap();
        let encoded = serde_json::to_value(view).unwrap();
        assert_eq!(encoded["tls"]["material_ref"], "managed:web");
        assert_eq!(encoded["tls"]["material_configured"], true);
        assert!(encoded["tls"].get("certificate_path").is_none());
        assert!(encoded["tls"].get("private_key_path").is_none());

        let patch = ProductNetworkSettingsPatch {
            schema_version: PRODUCT_NETWORK_SCHEMA_VERSION,
            bind: lan().bind,
            allowed_hosts: lan().allowed_hosts,
            tls: Some(ProductNetworkTlsSettingsPatch {
                schema_version: PRODUCT_NETWORK_SCHEMA_VERSION,
                material_ref: PRODUCT_NETWORK_MATERIAL_REF.into(),
            }),
            firewall: lan().firewall,
        };
        let encoded = serde_json::to_value(patch).unwrap();
        assert!(encoded.get("certificate_path").is_none());
        assert!(encoded.get("private_key_path").is_none());
        assert!(encoded["tls"].get("certificate_path").is_none());
        assert!(encoded["tls"].get("private_key_path").is_none());
    }
}
