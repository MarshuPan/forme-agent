//! Read-only SCM and image checks shared by the audited native transport.
#![cfg(windows)]
#![forbid(unsafe_code)]

use std::{
    fs::{self, File},
    os::windows::io::OwnedHandle,
    path::{Path, PathBuf},
};

use super::windows_agent_execution_native as native;
use forme_protocol as p;
use windows_service::service::{ServiceAccess, ServiceState, ServiceType};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

/// Managed listener lifecycle; all Win32 calls remain in the native adapter.
pub struct WindowsAgentExecutionListener {
    name: String,
    expected_image: PathBuf,
    pending: Option<OwnedHandle>,
}

impl WindowsAgentExecutionListener {
    pub fn bind(install_id: &str, expected_image: &Path) -> p::Result<Self> {
        let name = windows_agent_execution_pipe_name(install_id)?;
        if !expected_image.is_absolute() || !expected_image.is_file() {
            return Err(p::Error(
                "installed Brain image is unavailable for AgentExecution peer binding".into(),
            ));
        }
        let expected_image = fs::canonicalize(expected_image).map_err(|_| {
            p::Error("installed Brain image cannot be canonicalized for peer binding".into())
        })?;
        native::prepare_host_identity_query()?;
        let pending = Some(native::create_agent_execution_pipe(&name, true)?);
        Ok(Self {
            name,
            expected_image,
            pending,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Rejects an unverified peer without losing ownership of the namespace.
    pub fn try_accept(&mut self) -> p::Result<Option<File>> {
        let handle = self
            .pending
            .take()
            .ok_or_else(|| p::Error("AgentExecution pipe state is unavailable".into()))?;
        match native::poll_connection(&handle)? {
            native::PipeConnection::Listening => {
                self.pending = Some(handle);
                return Ok(None);
            }
            native::PipeConnection::Disconnected => {
                self.pending = Some(native::create_agent_execution_pipe(&self.name, false)?);
                return Ok(None);
            }
            native::PipeConnection::Connected => {}
        }
        // Claim the next instance before handing off or rejecting this one.
        self.pending = Some(native::create_agent_execution_pipe(&self.name, false)?);
        if !matches!(
            native::verify_agent_execution_peer(&handle, &self.expected_image),
            Ok(true)
        ) {
            return Ok(None);
        }
        native::finish_connection(handle).map(Some)
    }

    #[cfg(test)]
    pub(super) fn fixture(name: String, expected_image: PathBuf, pending: OwnedHandle) -> Self {
        Self {
            name,
            expected_image,
            pending: Some(pending),
        }
    }

    #[cfg(test)]
    pub(super) fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

/// Opens a stream before any caller can write credentials or a job.
pub fn open_agent_execution_pipe(name: &str) -> p::Result<File> {
    if !name.starts_with(r"\\.\pipe\forme-agent-execution-v1-")
        || name.len() > 256
        || name.chars().any(char::is_control)
    {
        return Err(p::Error("AgentExecution pipe name is invalid".into()));
    }
    let file = native::open_nonblocking_pipe(name)?;
    native::verify_agent_execution_server(&file)?;
    Ok(file)
}

/// Derives the installed, non-user-selectable AgentExecution pipe name.
pub fn windows_agent_execution_pipe_name(install_id: &str) -> p::Result<String> {
    p::validate_product_id(install_id)?;
    let digest = p::canonical_digest(&("forme-agent-execution-pipe-v1", install_id))?.0;
    let digest = digest
        .strip_prefix("sha256:")
        .ok_or_else(|| p::Error("agent execution pipe digest is malformed".into()))?;
    Ok(format!(r"\\.\pipe\forme-agent-execution-v1-{digest}"))
}

pub(super) fn verify_host_image(pid: u32, observed: PathBuf) -> p::Result<()> {
    let invalid = || p::Error("AgentExecution Host service identity is unverifiable".into());
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|_| invalid())?;
    let service = manager
        .open_service(
            p::BuiltinServiceId::Host.service_name(),
            ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG,
        )
        .map_err(|_| invalid())?;
    let status = service.query_status().map_err(|_| invalid())?;
    let config = service.query_config().map_err(|_| invalid())?;
    let account = config.account_name.as_deref().ok_or_else(invalid)?;
    if status.current_state != ServiceState::Running
        || status.process_id != Some(pid)
        || status.service_type != ServiceType::OWN_PROCESS
        || config.service_type != ServiceType::OWN_PROCESS
        || !matches!(account.to_str(), Some(name) if name.eq_ignore_ascii_case("LocalSystem") || name.eq_ignore_ascii_case(r"NT AUTHORITY\SYSTEM"))
    {
        return Err(invalid());
    }
    let core_image = std::env::current_exe().map_err(|_| invalid())?;
    let expected = core_image
        .parent()
        .ok_or_else(invalid)?
        .join("forme-hostd.exe");
    super::reject_linked_ancestors(&expected)?;
    let expected = fs::canonicalize(expected).map_err(|_| invalid())?;
    let observed = fs::canonicalize(observed).map_err(|_| invalid())?;
    if !observed
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.to_string_lossy())
    {
        return Err(invalid());
    }
    Ok(())
}
