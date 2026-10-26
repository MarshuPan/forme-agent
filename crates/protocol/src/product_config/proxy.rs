//! Host-owned model transport proxy configuration.
use serde::{Deserialize, Serialize};

use crate::{Error, Result, SchemaVersion};

use super::PRODUCT_PROXY_SCHEMA_VERSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductProxyMode {
    Direct,
    Http,
}

/// The endpoint is accepted only in an owner-authenticated patch and is never
/// included in the public settings projection.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductProxyConfig {
    pub schema_version: SchemaVersion,
    pub mode: ProductProxyMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

impl std::fmt::Debug for ProductProxyConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductProxyConfig")
            .field("schema_version", &self.schema_version)
            .field("mode", &self.mode)
            .field("endpoint", &self.endpoint.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

impl ProductProxyConfig {
    pub fn direct() -> Self {
        Self {
            schema_version: PRODUCT_PROXY_SCHEMA_VERSION,
            mode: ProductProxyMode::Direct,
            endpoint: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_PROXY_SCHEMA_VERSION {
            return Err(Error("product model proxy configuration is invalid".into()));
        }
        match self.mode {
            ProductProxyMode::Direct if self.endpoint.is_none() => Ok(()),
            ProductProxyMode::Http => {
                let endpoint = self
                    .endpoint
                    .as_deref()
                    .ok_or_else(|| Error("product HTTP proxy endpoint is missing".into()))?;
                if endpoint.is_empty()
                    || endpoint.len() > 2048
                    || endpoint.chars().any(char::is_control)
                {
                    return Err(Error("product HTTP proxy endpoint is invalid".into()));
                }
                Ok(())
            }
            ProductProxyMode::Direct => Err(Error(
                "direct product model proxy must not include an endpoint".into(),
            )),
        }
    }
}

/// Safe public projection of the proxy setting. It deliberately carries no
/// endpoint, credential, or secret reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductProxySettingsView {
    pub schema_version: SchemaVersion,
    pub mode: ProductProxyMode,
    pub endpoint_configured: bool,
}

impl ProductProxySettingsView {
    pub fn from_internal(proxy: &ProductProxyConfig) -> Result<Self> {
        proxy.validate()?;
        Ok(Self {
            schema_version: PRODUCT_PROXY_SCHEMA_VERSION,
            mode: proxy.mode,
            endpoint_configured: proxy.endpoint.is_some(),
        })
    }
}
