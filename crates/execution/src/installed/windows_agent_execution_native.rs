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
    ERROR_NO_DATA, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, ERROR_PIPE_NOT_CONNECTED,
    ERROR_SUCCESS, HANDLE, HLOCAL, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    BuildTrusteeWithSidW, ConvertSidToStringSidW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetEffectiveRightsFromAclW,
    GetSecurityInfo, SetEntriesInAclW, SetSecurityInfo, EXPLICIT_ACCESS_W, GRANT_ACCESS,
    SE_KERNEL_OBJECT, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    CheckTokenMembership, DuplicateToken, EqualSid, GetTokenInformation, IsValidSid,
    LookupAccountNameW, SecurityIdentification, TokenUser, ACL, DACL_SECURITY_INFORMATION, PSID,
    SECURITY_ATTRIBUTES, SID_NAME_USE, TOKEN_DUPLICATE, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, READ_CONTROL, SECURITY_IDENTIFICATION,
    WRITE_DAC,
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

pub use super::windows_agent_execution_identity::{
    open_agent_execution_pipe, windows_agent_execution_pipe_name, WindowsAgentExecutionListener,
};
use super::windows_appcontainer_support::wide;

const BRAIN_SERVICE_ACCOUNT: &str = "NT SERVICE\\forme-brain";
const HOST_SERVICE_ACCOUNT: &str = "NT SERVICE\\forme-host";
// Sixteen handler slots plus the accepted and replacement listener instances.
const MAX_PIPE_INSTANCES: u32 = 18;
const PIPE_BUFFER_BYTES: u32 = 64 * 1024;

pub(super) fn prepare_host_identity_query() -> p::Result<()> {
    grant_object_access(
        &current_process()?,
        &lookup_account_sid(BRAIN_SERVICE_ACCOUNT)?,
        PROCESS_QUERY_LIMITED_INFORMATION,
    )
}

/// Prepares only the current Brain's kernel objects for the fixed Host peer.
/// This never changes files, service configuration, or action authorization.
pub(super) fn prepare_brain_identity_query() -> p::Result<()> {
    let process = current_process()?;
    let token = identity_query_token(&process)?;
    let brain = lookup_account_sid(BRAIN_SERVICE_ACCOUNT)?;
    let mut length = 0;
    // SAFETY: the null buffer requests only the required, bounded byte count.
    unsafe { GetTokenInformation(raw(&token), TokenUser, null_mut(), 0, &mut length) };
    if length < size_of::<TOKEN_USER>() as u32 || length > 1024 * 1024 {
        return Err(native_error("Brain token user cannot be inspected"));
    }
    let mut user = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
    // SAFETY: usize storage supplies TOKEN_USER alignment and the full required buffer.
    if unsafe {
        GetTokenInformation(
            raw(&token),
            TokenUser,
            user.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
        || unsafe {
            EqualSid(
                (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid,
                brain.raw(),
            )
        } == 0
    {
        return Err(native_error(
            "managed worker caller is not the Brain service account",
        ));
    }
    let host = lookup_account_sid(HOST_SERVICE_ACCOUNT)?;
    grant_object_access(&process, &host, PROCESS_QUERY_LIMITED_INFORMATION)?;
    grant_object_access(&token, &host, TOKEN_QUERY | TOKEN_DUPLICATE)
}

fn identity_query_token(process: &OwnedHandle) -> p::Result<OwnedHandle> {
    let mut token = null_mut();
    // SAFETY: process is owned; the output receives a new, uniquely owned token.
    if unsafe {
        OpenProcessToken(
            raw(process),
            TOKEN_QUERY | READ_CONTROL | WRITE_DAC,
            &mut token,
        )
    } == 0
    {
        return Err(native_error("Brain identity query token is unavailable"));
    }
    owned_handle(token, "Brain identity query token is invalid")
}

fn current_process() -> p::Result<OwnedHandle> {
    // SAFETY: only this process is opened, without inheritance or execution rights.
    let handle = unsafe {
        OpenProcess(
            READ_CONTROL | WRITE_DAC | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            std::process::id(),
        )
    };
    owned_handle(handle, "current peer process security is unavailable")
}

struct LocalSecurity(*mut c_void);
impl Drop for LocalSecurity {
    fn drop(&mut self) {
        // SAFETY: this uniquely owns memory returned by a LocalAlloc Win32 API.
        if !self.0.is_null() {
            unsafe { LocalFree(self.0 as HLOCAL) };
        }
    }
}

fn object_dacl(object: &OwnedHandle) -> p::Result<(LocalSecurity, *mut ACL)> {
    let mut acl = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: object is live and all requested outputs are writable pointers.
    let status = unsafe {
        GetSecurityInfo(
            raw(object),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut acl,
            null_mut(),
            &mut descriptor,
        )
    };
    let descriptor = LocalSecurity(descriptor);
    if status != ERROR_SUCCESS || acl.is_null() || descriptor.0.is_null() {
        return Err(native_error(
            "peer kernel object DACL is unavailable or unrestricted",
        ));
    }
    Ok((descriptor, acl))
}

fn object_rights(object: &OwnedHandle, sid: &SidBuffer) -> p::Result<u32> {
    let (_descriptor, acl) = object_dacl(object)?;
    let mut trustee = TRUSTEE_W::default();
    let mut rights = 0;
    // SAFETY: SID and descriptor remain live throughout the read-only query.
    unsafe { BuildTrusteeWithSidW(&mut trustee, sid.raw()) };
    if unsafe { GetEffectiveRightsFromAclW(acl, &trustee, &mut rights) } != ERROR_SUCCESS {
        return Err(native_error("peer kernel object access cannot be verified"));
    }
    Ok(rights)
}

fn grant_object_access(object: &OwnedHandle, sid: &SidBuffer, rights: u32) -> p::Result<()> {
    let (_descriptor, old_acl) = object_dacl(object)?;
    let mut trustee = TRUSTEE_W::default();
    // SAFETY: the owned SID outlives the synchronous ACL merge.
    unsafe { BuildTrusteeWithSidW(&mut trustee, sid.raw()) };
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: rights,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: 0,
        Trustee: trustee,
    };
    let mut new_acl = null_mut();
    // SAFETY: one initialized entry is merged with the still-owned original DACL.
    let status = unsafe { SetEntriesInAclW(1, &entry, old_acl, &mut new_acl) };
    let new_acl_owner = LocalSecurity(new_acl.cast());
    if status != ERROR_SUCCESS || new_acl_owner.0.is_null() {
        return Err(native_error(
            "peer kernel object query ACL cannot be merged",
        ));
    }
    // SAFETY: changes only this already-owned object's DACL, preserving other ACEs.
    if unsafe {
        SetSecurityInfo(
            raw(object),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            new_acl,
            null_mut(),
        )
    } != ERROR_SUCCESS
        || object_rights(object, sid)? & rights != rights
    {
        return Err(native_error(
            "peer kernel object query ACL could not be verified",
        ));
    }
    Ok(())
}

pub(super) enum PipeConnection {
    Listening,
    Disconnected,
    Connected,
}

pub(super) fn poll_connection(handle: &OwnedHandle) -> p::Result<PipeConnection> {
    // SAFETY: the owned polling handle is live; no overlapped operation is used.
    unsafe { SetLastError(0) };
    let connected = unsafe { ConnectNamedPipe(raw(handle), null_mut()) } != 0;
    let error = unsafe { GetLastError() };
    match (connected, error) {
        (true, _) | (false, ERROR_PIPE_CONNECTED) => Ok(PipeConnection::Connected),
        (false, ERROR_PIPE_LISTENING) => Ok(PipeConnection::Listening),
        (false, ERROR_NO_DATA) => Ok(PipeConnection::Disconnected),
        _ => Err(native_error(
            "AgentExecution named pipe connection state is unknown",
        )),
    }
}

pub(super) fn finish_connection(handle: OwnedHandle) -> p::Result<File> {
    let mode = PIPE_WAIT;
    // SAFETY: the uniquely owned pipe is connected and mode is a live u32.
    if unsafe { SetNamedPipeHandleState(raw(&handle), &mode, null(), null()) } == 0 {
        return Err(native_error(
            "AgentExecution named pipe could not enter blocking mode",
        ));
    }
    // SAFETY: transfer this uniquely owned handle exactly once into File.
    Ok(unsafe { File::from_raw_handle(handle.into_raw_handle()) })
}

pub(super) fn open_nonblocking_pipe(name: &str) -> p::Result<File> {
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

pub(super) fn verify_agent_execution_server(file: &File) -> p::Result<()> {
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
    super::windows_agent_execution_identity::verify_host_image(
        pid,
        process_image_path(raw(&process))?,
    )
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

pub(super) fn create_agent_execution_pipe(name: &str, first: bool) -> p::Result<OwnedHandle> {
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

pub(super) fn verify_agent_execution_peer(
    handle: &OwnedHandle,
    expected_image: &Path,
) -> p::Result<bool> {
    let pipe = raw(handle);
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

#[cfg(test)]
fn fixture_process(pid: u32) -> p::Result<OwnedHandle> {
    // SAFETY: the caller owns the fixture child; no execution rights are requested.
    owned_handle(
        unsafe {
            OpenProcess(
                READ_CONTROL | WRITE_DAC | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        },
        "fixture process unavailable",
    )
}

#[cfg(test)]
fn fixture_acl_bytes(object: &OwnedHandle) -> p::Result<Vec<u8>> {
    let (_descriptor, acl) = object_dacl(object)?;
    // SAFETY: the descriptor owns this valid ACL and its complete size.
    let length = unsafe { (*acl).AclSize as usize };
    if length < size_of::<ACL>() {
        return Err(native_error("fixture ACL is malformed"));
    }
    Ok(unsafe { std::slice::from_raw_parts(acl.cast::<u8>(), length) }.to_vec())
}

#[cfg(test)]
#[path = "windows_agent_execution_tests.rs"]
mod tests;
