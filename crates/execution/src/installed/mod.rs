//! Host-management storage and platform adapters, independent of brain state.
#![deny(unsafe_code)]

pub mod bootstrap;
mod desktop_authority;
mod desktop_helper;
#[cfg(test)]
mod desktop_output_fixture;
mod desktop_output_sink;
mod desktop_output_storage;
pub use desktop_output_sink::DesktopOutputSink;
mod desktop_provisioning;
pub use desktop_provisioning::{
    InstalledDesktopProvisioning, INSTALLED_DESKTOP_PROFILE, INSTALLED_DESKTOP_SURFACE,
};
#[cfg(windows)]
mod desktop_helper_wire;
#[cfg(windows)]
mod desktop_output_release;
mod file_provisioning;
mod file_worker;
#[cfg(windows)]
mod firewall_windows;
mod host;
mod journal;
mod legacy_data_archive;
#[cfg(target_os = "linux")]
mod linux_worker;
#[cfg(target_os = "macos")]
mod mac_worker;
#[cfg(target_os = "macos")]
mod mac_worker_sandbox;
mod maintenance_pipe;
#[cfg(windows)]
mod managed_mcp_program;
#[cfg(windows)]
mod managed_mcp_proof;
#[cfg(windows)]
mod managed_mcp_stdio;
mod managed_worker;
#[cfg(windows)]
pub use managed_mcp_program::{
    InstalledManagedMcpPrograms, ManagedMcpProgramLease, ManagedMcpProgramRegistry,
    ManagedMcpProgramSource,
};
mod managed_mcp_material;
mod managed_mcp_source;
pub use managed_mcp_source::ManagedMcpSourceVerifier;
mod channel_delivery;
mod management_repair;
mod paths;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod posix_worker_fs;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod posix_worker_overlay;
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
mod windows_agent_execution_identity;
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
mod windows_desktop_helper_peer_native;
#[cfg(windows)]
mod windows_desktop_helper_runtime;
#[cfg(windows)]
mod windows_desktop_output_runtime;
#[cfg(windows)]
mod windows_desktop_session_native;
#[cfg(windows)]
mod windows_worker;
#[cfg(windows)]
mod windows_workspace_overlay;
mod worker;
mod worker_backend;
mod worker_binding;
mod worker_client;
mod worker_config;
mod worker_host;
mod worker_input;
mod worker_isolation;
#[cfg(windows)]
mod worker_pipe;
mod worker_result_crypto;
mod worker_results;
mod worker_socket;
#[cfg(windows)]
mod worker_stdio;
pub use channel_delivery::InstalledChannelDelivery;

#[cfg(windows)]
pub use desktop_authority::WindowsDesktopSessionCatalog;
pub use desktop_authority::{
    DesktopAuthority, DesktopSessionCatalogProvider, InstalledDesktopAuthority,
    ObservedDesktopSession,
};
#[cfg(windows)]
pub use desktop_helper::WindowsDesktopHelperAdapter;
pub use desktop_helper::{DesktopHelperAdapter, UnavailableDesktopHelper};
pub use file_provisioning::{InstalledFileProvisioning, INSTALLED_FILE_PROFILE};
pub use file_worker::run_installed_file_worker;
pub use host::*;
pub use journal::*;
#[cfg(target_os = "linux")]
pub use linux_worker::*;
#[cfg(target_os = "macos")]
pub use mac_worker::*;
pub use managed_mcp_material::*;
pub use managed_worker::*;
pub use management_repair::{ManagementReaderRepairEvidence, ManagementReaderRepairInspection};
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
pub use windows_desktop_helper_runtime::run_desktop_session_helper;
#[cfg(windows)]
pub use windows_desktop_output_runtime::run_desktop_output_helper;
#[cfg(windows)]
pub use windows_worker::*;
pub use worker::*;
pub use worker_backend::*;
pub use worker_client::*;
pub use worker_config::*;
pub use worker_host::*;
pub use worker_input::*;
