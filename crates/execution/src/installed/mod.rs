//! Host-management storage and platform adapters, independent of brain state.
#![deny(unsafe_code)]

pub mod bootstrap;
#[cfg(windows)]
mod firewall_windows;
mod host;
mod journal;
mod maintenance_pipe;
mod paths;
mod process;
mod product_config;
#[cfg(windows)]
mod registration;
mod release;
mod release_data_worker;
mod release_probe;
mod release_services;
mod release_staging;
mod release_updater;
mod release_worker;
mod restore_worker;
mod sandbox_lease;
mod service;
mod web_health;
#[cfg(windows)]
mod windows_agent_execution_native;
#[cfg(windows)]
mod windows_appcontainer;
#[cfg(windows)]
mod windows_appcontainer_acl;
#[cfg(windows)]
mod windows_appcontainer_native;
#[cfg(windows)]
mod windows_appcontainer_support;
#[cfg(windows)]
mod windows_worker;
#[cfg(windows)]
mod windows_workspace_overlay;
mod worker;
mod worker_binding;
mod worker_client;
mod worker_config;
mod worker_host;

pub use host::*;
pub use journal::*;
pub use paths::*;
pub use process::*;
pub use product_config::*;
#[cfg(windows)]
pub use registration::process::{bind_windows_host_service, WindowsHostServiceBinding};
#[cfg(windows)]
pub use registration::updater::{
    register as register_windows_release_updater, unregister as unregister_windows_release_updater,
};
#[cfg(windows)]
pub use registration::{register_windows_service, unregister_windows_service};
pub use release::*;
pub use release_data_worker::*;
pub use release_services::*;
pub use release_staging::*;
pub use release_updater::*;
pub use release_worker::*;
pub use restore_worker::*;
pub use sandbox_lease::*;
pub use service::*;
#[cfg(windows)]
pub use windows_agent_execution_native::{
    open_agent_execution_pipe, windows_agent_execution_pipe_name, WindowsAgentExecutionListener,
};
#[cfg(windows)]
pub use windows_appcontainer::*;
#[cfg(windows)]
pub use windows_worker::*;
pub use worker::*;
pub use worker_client::*;
pub use worker_config::*;
pub use worker_host::*;
