#![cfg(windows)]
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::{self, File, OpenOptions};
use std::mem::size_of;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use forme_protocol as p;
use windows_sys::Win32::Foundation::{
    GetLastError, LocalFree, SetLastError, ERROR_BROKEN_PIPE, ERROR_INSUFFICIENT_BUFFER,
    ERROR_NO_DATA, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, ERROR_PIPE_NOT_CONNECTED, HANDLE,
    HLOCAL, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows_sys::Win32::Security::{
    CheckTokenMembership, DuplicateToken, IsValidSid, LookupAccountNameW, SecurityIdentification,
    PSID, SECURITY_ATTRIBUTES, SID_NAME_USE, TOKEN_DUPLICATE, TOKEN_QUERY,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
    PeekNamedPipe, SetNamedPipeHandleState, WaitNamedPipeW, PIPE_NOWAIT, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::windows_appcontainer_support::wide;

const BRAIN_SERVICE_ACCOUNT: &str = "NT SERVICE\\forme-brain";
// Sixteen handler slots plus the accepted and replacement listener instances.
const MAX_PIPE_INSTANCES: u32 = 18;
const PIPE_BUFFER_BYTES: u32 = 64 * 1024;

/// Derives the installed, non-user-selectable AgentExecution pipe name.
pub fn windows_agent_execution_pipe_name(install_id: &str) -> p::Result<String> {
    p::validate_product_id(install_id)?;
    let digest = p::canonical_digest(&("forme-agent-execution-pipe-v1", install_id))?.0;
    let digest = digest
        .strip_prefix("sha256:")
        .ok_or_else(|| p::Error("agent execution pipe digest is malformed".into()))?;
    Ok(format!(r"\\.\pipe\forme-agent-execution-v1-{digest}"))
}

/// The native AgentExecution server used by a managed Windows Host.
///
/// It polls a synchronous `PIPE_NOWAIT` instance so the Host supervisor keeps
/// ownership of shutdown and connection admission. Every accepted instance is
/// verified before it is handed to the HTTP framing layer.
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
        let _ = lookup_account_sid(BRAIN_SERVICE_ACCOUNT)?;
        let pending = Some(create_agent_execution_pipe(&name, true)?);
        Ok(Self {
            name,
            expected_image: fs::canonicalize(expected_image).map_err(|_| {
                p::Error("installed Brain image cannot be canonicalized for peer binding".into())
            })?,
            pending,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns one verified Brain connection, or `None` when no client is
    /// waiting. A rejected peer is dropped without exposing the HTTP layer.
    pub fn try_accept(&mut self) -> p::Result<Option<File>> {
        let handle = self
            .pending
            .take()
            .ok_or_else(|| p::Error("AgentExecution pipe state is unavailable".into()))?;
        unsafe { SetLastError(0) };
        let connected = unsafe { ConnectNamedPipe(raw(&handle), null_mut()) } != 0;
        let error = unsafe { GetLastError() };
        if !connected && error == ERROR_PIPE_LISTENING {
            self.pending = Some(handle);
            return Ok(None);
        }
        if !connected && error == ERROR_NO_DATA {
            // Keep the namespace claimed while replacing a disconnected peer.
            self.pending = Some(create_agent_execution_pipe(&self.name, false)?);
            return Ok(None);
        }
        if !connected && error != ERROR_PIPE_CONNECTED {
            return Err(native_error(
                "AgentExecution named pipe connection state is unknown",
            ));
        }
        // Claim the next instance before handing off or rejecting this one.
        // FIRST_PIPE_INSTANCE is only valid for initial namespace ownership.
        self.pending = Some(create_agent_execution_pipe(&self.name, false)?);
        if !matches!(
            verify_agent_execution_peer(raw(&handle), &self.expected_image),
            Ok(true)
        ) {
            return Ok(None);
        }
        let mode = PIPE_WAIT;
        if unsafe { SetNamedPipeHandleState(raw(&handle), &mode, null(), null()) } == 0 {
            return Err(native_error(
                "AgentExecution named pipe could not enter blocking mode",
            ));
        }
        let raw_handle = handle.into_raw_handle();
        // SAFETY: ownership is transferred exactly once from OwnedHandle to File.
        Ok(Some(unsafe { File::from_raw_handle(raw_handle) }))
    }
}

/// Opens a mutually authenticated stream. No caller can write credentials
/// before the fixed Host service PID, identity and release image are verified.
pub fn open_agent_execution_pipe(name: &str) -> p::Result<File> {
    if !name.starts_with(r"\\.\pipe\forme-agent-execution-v1-")
        || name.len() > 256
        || name.chars().any(char::is_control)
    {
        return Err(p::Error("AgentExecution pipe name is invalid".into()));
    }
    let file = open_nonblocking_pipe(name)?;
    verify_agent_execution_server(&file)?;
    Ok(file)
}

fn open_nonblocking_pipe(name: &str) -> p::Result<File> {
    let name_w = wide(name);
    if unsafe { WaitNamedPipeW(name_w.as_ptr(), 30_000) } == 0 {
        return Err(native_error("AgentExecution named pipe is unavailable"));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .security_qos_flags(SECURITY_IDENTIFICATION)
        .open(name)
        .map_err(|_| p::Error("AgentExecution named pipe could not be opened".into()))?;
    let mode = PIPE_READMODE_BYTE | PIPE_NOWAIT;
    // SAFETY: file owns a connected duplex pipe; mode points to a live u32,
    // and both unused optional pointers are null. The safe client applies
    // its absolute I/O deadline while polling this nonblocking handle.
    if unsafe { SetNamedPipeHandleState(file.as_raw_handle() as HANDLE, &mode, null(), null()) }
        == 0
    {
        return Err(native_error(
            "AgentExecution pipe could not enter bounded I/O mode",
        ));
    }
    Ok(file)
}

fn verify_agent_execution_server(file: &File) -> p::Result<()> {
    use windows_service::service::{ServiceAccess, ServiceState, ServiceType};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    let invalid = || p::Error("AgentExecution Host service identity is unverifiable".into());
    let mut pid = 0;
    // SAFETY: file owns the connected pipe and pid is a valid output buffer.
    if unsafe { GetNamedPipeServerProcessId(file.as_raw_handle() as HANDLE, &mut pid) } == 0
        || pid == 0
    {
        return Err(invalid());
    }
    // SAFETY: PID comes from the live pipe; only limited read access is requested.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    let process = owned_handle(process, "AgentExecution Host process cannot be opened")?;
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
    let expected_image = core_image
        .parent()
        .ok_or_else(invalid)?
        .join("forme-hostd.exe");
    super::reject_linked_ancestors(&expected_image)?;
    let expected_image = fs::canonicalize(expected_image).map_err(|_| invalid())?;
    let observed_image =
        fs::canonicalize(process_image_path(raw(&process))?).map_err(|_| invalid())?;
    if !observed_image
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected_image.to_string_lossy())
    {
        return Err(invalid());
    }
    Ok(())
}

/// Distinguishes a connected-but-empty byte pipe from a closed peer. Rust's
/// File::read can report zero bytes for both cases on a PIPE_NOWAIT handle.
pub(super) fn agent_execution_pipe_available(file: &File) -> std::io::Result<Option<u32>> {
    let mut available = 0;
    // SAFETY: file owns the live pipe handle, no data buffer is requested,
    // available is a valid writable u32 and other output pointers are null.
    if unsafe {
        PeekNamedPipe(
            file.as_raw_handle() as HANDLE,
            null_mut(),
            0,
            null_mut(),
            &mut available,
            null_mut(),
        )
    } != 0
    {
        return Ok(Some(available));
    }
    // SAFETY: capture the failure immediately after PeekNamedPipe.
    let error = unsafe { GetLastError() };
    if matches!(error, ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED) {
        Ok(None)
    } else {
        Err(std::io::Error::from_raw_os_error(error as i32))
    }
}

fn create_agent_execution_pipe(name: &str, first: bool) -> p::Result<OwnedHandle> {
    let security = PipeSecurity::new(BRAIN_SERVICE_ACCOUNT)?;
    create_pipe_instance(name, first, Some(&security))
}

fn create_pipe_instance(
    name: &str,
    first: bool,
    security: Option<&PipeSecurity>,
) -> p::Result<OwnedHandle> {
    let name_w = wide(name);
    let handle = unsafe {
        CreateNamedPipeW(
            name_w.as_ptr(),
            PIPE_ACCESS_DUPLEX
                | if first {
                    FILE_FLAG_FIRST_PIPE_INSTANCE
                } else {
                    0
                },
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            MAX_PIPE_INSTANCES,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            30_000,
            security.map_or(null(), |security| &security.attributes),
        )
    };
    owned_handle(handle, "AgentExecution named pipe could not be created")
}

fn verify_agent_execution_peer(pipe: HANDLE, expected_image: &Path) -> p::Result<bool> {
    let mut pid = 0u32;
    if unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) } == 0 || pid == 0 {
        return Err(native_error(
            "AgentExecution named pipe client PID is unavailable",
        ));
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    let process = owned_handle(process, "AgentExecution client process cannot be opened")?;
    let mut token = null_mut();
    if unsafe { OpenProcessToken(raw(&process), TOKEN_QUERY | TOKEN_DUPLICATE, &mut token) } == 0 {
        return Err(native_error("AgentExecution client token cannot be opened"));
    }
    let token = owned_handle(token, "AgentExecution client token handle is invalid")?;
    let mut identification = null_mut();
    // SAFETY: token is an owned primary token with duplicate/query access;
    // membership checks require an impersonation token, never a primary token.
    if unsafe { DuplicateToken(raw(&token), SecurityIdentification, &mut identification) } == 0 {
        return Err(native_error(
            "AgentExecution client token cannot be duplicated",
        ));
    }
    let identification = owned_handle(
        identification,
        "AgentExecution identification token is invalid",
    )?;
    let sid = lookup_account_sid(BRAIN_SERVICE_ACCOUNT)?;
    let mut member = 0;
    if unsafe { CheckTokenMembership(raw(&identification), sid.raw(), &mut member) } == 0 {
        return Err(native_error(
            "AgentExecution client service SID membership cannot be checked",
        ));
    }
    if member == 0 {
        return Ok(false);
    }
    let observed_image = process_image_path(raw(&process))?;
    let observed_image = fs::canonicalize(observed_image)
        .map_err(|_| p::Error("AgentExecution client image cannot be canonicalized".into()))?;
    Ok(observed_image
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected_image.to_string_lossy()))
}

fn process_image_path(process: HANDLE) -> p::Result<PathBuf> {
    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &mut length,
        )
    } == 0
        || length == 0
        || length as usize > buffer.len()
    {
        return Err(native_error(
            "AgentExecution client image path is unavailable",
        ));
    }
    String::from_utf16(&buffer[..length as usize])
        .map(PathBuf::from)
        .map_err(|_| p::Error("AgentExecution client image path is invalid".into()))
}

struct SidBuffer {
    bytes: Vec<u8>,
}

impl SidBuffer {
    fn raw(&self) -> PSID {
        self.bytes.as_ptr() as PSID
    }
}

fn lookup_account_sid(account: &str) -> p::Result<SidBuffer> {
    let account_w = wide(account);
    let mut sid_bytes = 0u32;
    let mut domain_chars = 0u32;
    let mut use_type = SID_NAME_USE::default();
    unsafe {
        LookupAccountNameW(
            null(),
            account_w.as_ptr(),
            null_mut(),
            &mut sid_bytes,
            null_mut(),
            &mut domain_chars,
            &mut use_type,
        );
    }
    if sid_bytes == 0 || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
        return Err(native_error("Brain Service SID cannot be resolved"));
    }
    let mut sid = vec![0u8; sid_bytes as usize];
    let mut domain = vec![0u16; (domain_chars as usize).max(1)];
    let ok = unsafe {
        LookupAccountNameW(
            null(),
            account_w.as_ptr(),
            sid.as_mut_ptr() as PSID,
            &mut sid_bytes,
            domain.as_mut_ptr(),
            &mut domain_chars,
            &mut use_type,
        )
    };
    if ok == 0 || unsafe { IsValidSid(sid.as_mut_ptr() as PSID) } == 0 {
        return Err(native_error("Brain Service SID is invalid"));
    }
    sid.truncate(sid_bytes as usize);
    Ok(SidBuffer { bytes: sid })
}

struct PipeSecurity {
    descriptor: *mut c_void,
    attributes: SECURITY_ATTRIBUTES,
}

impl PipeSecurity {
    fn new(account: &str) -> p::Result<Self> {
        let sid = lookup_account_sid(account)?;
        let mut sid_text = null_mut();
        if unsafe { ConvertSidToStringSidW(sid.raw(), &mut sid_text) } == 0 || sid_text.is_null() {
            return Err(native_error("Brain Service SID string cannot be resolved"));
        }
        let text = (|| {
            let mut len = 0usize;
            while len < 184 && unsafe { *sid_text.add(len) } != 0 {
                len += 1;
            }
            if len == 184 {
                return Err(native_error("Brain Service SID string is unbounded"));
            }
            String::from_utf16(unsafe { std::slice::from_raw_parts(sid_text, len) })
                .map_err(|_| native_error("Brain Service SID string is invalid"))
        })();
        unsafe { LocalFree(sid_text as HLOCAL) };
        let sid_text = text?;
        let sddl = wide(&format!("D:P(A;;GA;;;SY)(A;;GRGW;;;{sid_text})"));
        let mut descriptor = null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
            || descriptor.is_null()
        {
            return Err(native_error(
                "AgentExecution named pipe security descriptor cannot be created",
            ));
        }
        Ok(Self {
            descriptor,
            attributes: SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            },
        })
    }
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        if !self.descriptor.is_null() {
            unsafe { LocalFree(self.descriptor as HLOCAL) };
        }
    }
}

fn owned_handle(handle: HANDLE, message: &'static str) -> p::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(native_error(message));
    }
    // SAFETY: only a successful, uniquely owned Win32 handle reaches this point.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle() as HANDLE
}

fn native_error(message: &str) -> p::Error {
    p::Error(message.into())
}

// Current-user byte-stream helpers do not assert installed peer/SID identity.
#[cfg(test)]
fn fixture_listener() -> (OwnedHandle, String) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let name = format!(
        r"\\.\pipe\forme-agent-execution-v1-io-fixture-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    (create_pipe_instance(&name, true, None).unwrap(), name)
}

#[cfg(test)]
fn fixture_server(owned: OwnedHandle) -> File {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        // SAFETY: the unique nonblocking handle is live; no OVERLAPPED is used.
        let connected = unsafe { ConnectNamedPipe(raw(&owned), null_mut()) } != 0;
        let error = unsafe { GetLastError() };
        if connected || matches!(error, ERROR_PIPE_CONNECTED | ERROR_NO_DATA) {
            break;
        }
        assert_eq!(error, ERROR_PIPE_LISTENING);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // SAFETY: unique ownership transfers exactly once from OwnedHandle to File.
    unsafe { File::from_raw_handle(owned.into_raw_handle()) }
}

#[cfg(test)]
fn pipe_pair() -> (File, File) {
    let (owned, name) = fixture_listener();
    let client = open_nonblocking_pipe(&name).unwrap();
    (fixture_server(owned), client)
}

#[cfg(test)]
#[path = "windows_agent_execution_tests.rs"]
mod tests;
