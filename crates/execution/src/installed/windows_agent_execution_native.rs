#![cfg(windows)]
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::{self, File, OpenOptions};
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use forme_protocol as p;
use windows_sys::Win32::Foundation::{
    GetLastError, LocalFree, SetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_PIPE_CONNECTED,
    ERROR_PIPE_LISTENING, HANDLE, HLOCAL, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows_sys::Win32::Security::{
    CheckTokenMembership, IsValidSid, LookupAccountNameW, PSID, SECURITY_ATTRIBUTES, SID_NAME_USE,
    TOKEN_QUERY,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, SetNamedPipeHandleState,
    WaitNamedPipeW, PIPE_NOWAIT, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::windows_appcontainer_support::wide;

const BRAIN_SERVICE_ACCOUNT: &str = "NT SERVICE\\forme-brain";
const MAX_PIPE_INSTANCES: u32 = 16;
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
        Ok(Self {
            name,
            expected_image: fs::canonicalize(expected_image).map_err(|_| {
                p::Error("installed Brain image cannot be canonicalized for peer binding".into())
            })?,
            pending: None,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns one verified Brain connection, or `None` when no client is
    /// waiting. A rejected peer is dropped without exposing the HTTP layer.
    pub fn try_accept(&mut self) -> p::Result<Option<File>> {
        if self.pending.is_none() {
            self.pending = Some(create_agent_execution_pipe(&self.name)?);
        }
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
        if !connected && error != ERROR_PIPE_CONNECTED {
            return Err(native_error(
                "AgentExecution named pipe connection state is unknown",
            ));
        }
        if !verify_agent_execution_peer(raw(&handle), &self.expected_image)? {
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

/// Opens the managed AgentExecution pipe for the Brain process. The server
/// still verifies the caller's service SID and image; this helper only waits
/// for the fixed endpoint and opens a duplex byte stream.
pub fn open_agent_execution_pipe(name: &str) -> p::Result<File> {
    if !name.starts_with(r"\\.\pipe\forme-agent-execution-v1-")
        || name.len() > 256
        || name.chars().any(char::is_control)
    {
        return Err(p::Error("AgentExecution pipe name is invalid".into()));
    }
    let name_w = wide(name);
    if unsafe { WaitNamedPipeW(name_w.as_ptr(), 30_000) } == 0 {
        return Err(native_error("AgentExecution named pipe is unavailable"));
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(name)
        .map_err(|_| p::Error("AgentExecution named pipe could not be opened".into()))
}

fn create_agent_execution_pipe(name: &str) -> p::Result<OwnedHandle> {
    let security = PipeSecurity::new(BRAIN_SERVICE_ACCOUNT)?;
    let name_w = wide(name);
    let handle = unsafe {
        CreateNamedPipeW(
            name_w.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            MAX_PIPE_INSTANCES,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            30_000,
            &security.attributes,
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
    if unsafe { OpenProcessToken(raw(&process), TOKEN_QUERY, &mut token) } == 0 {
        return Err(native_error("AgentExecution client token cannot be opened"));
    }
    let token = owned_handle(token, "AgentExecution client token handle is invalid")?;
    let sid = lookup_account_sid(BRAIN_SERVICE_ACCOUNT)?;
    let mut member = 0;
    if unsafe { CheckTokenMembership(raw(&token), sid.raw(), &mut member) } == 0 {
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

#[cfg(test)]
mod tests {
    use super::windows_agent_execution_pipe_name;

    #[test]
    fn pipe_name_is_install_bound_and_closed() {
        let first = windows_agent_execution_pipe_name("managed-process-test").unwrap();
        let second = windows_agent_execution_pipe_name("another-installation").unwrap();
        assert_ne!(first, second);
        assert!(first.starts_with(r"\\.\pipe\forme-agent-execution-v1-"));
        assert!(windows_agent_execution_pipe_name("not a valid install").is_err());
    }
}
