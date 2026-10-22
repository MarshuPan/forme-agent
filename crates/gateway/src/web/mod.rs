//! Installed-product Web boundaries. Authentication is not a second brain.
pub(crate) mod config;
mod configuration;
mod core;
pub mod http;
pub mod identity;
mod tls;
pub use config::{
    WebBootstrapBridgeConfig, WebConfigurationBridgeConfig, WebCoreBridgeConfig, WebServerConfig,
};
pub use tls::WebTlsConfig;
pub mod bootstrap;
