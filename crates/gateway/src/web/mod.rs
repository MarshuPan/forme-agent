//! Installed-product Web boundaries. Authentication is not a second brain.
mod config;
pub mod http;
pub mod identity;
pub use config::WebServerConfig;
