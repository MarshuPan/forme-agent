//! Installed-product Web boundaries. Authentication is not a second brain.
pub(crate) mod config;
mod core;
pub mod http;
pub mod identity;
pub use config::{WebBootstrapBridgeConfig, WebCoreBridgeConfig, WebServerConfig};
pub mod bootstrap;
