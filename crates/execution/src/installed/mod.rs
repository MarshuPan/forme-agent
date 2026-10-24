//! Host-management storage and platform adapters, independent of brain state.
#![forbid(unsafe_code)]

pub mod bootstrap;
#[cfg(windows)]
mod firewall_windows;
mod host;
mod journal;
mod paths;
mod process;
mod product_config;
#[cfg(windows)]
mod registration;
mod release;
mod release_staging;
mod restore_worker;
mod service;
mod worker;
mod worker_config;

pub use host::*;
pub use journal::*;
pub use paths::*;
pub use process::*;
pub use product_config::*;
#[cfg(windows)]
pub use registration::{register_windows_service, unregister_windows_service};
pub use release::*;
pub use release_staging::*;
pub use restore_worker::*;
pub use service::*;
pub use worker::*;
pub use worker_config::*;
