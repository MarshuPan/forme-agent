#![cfg(windows)]
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::mem::size_of;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle, RawHandle};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use forme_protocol as p;
use serde::de::DeserializeOwned;
use serde::Serialize;
use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_BROKEN_PIPE, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING,
    ERROR_PIPE_NOT_CONNECTED, HANDLE, HLOCAL, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenSessionId, TokenStatistics, TokenUser, SECURITY_ATTRIBUTES,
    TOKEN_QUERY, TOKEN_STATISTICS, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PeekNamedPipe,
    SetNamedPipeHandleState, WaitNamedPipeW, PIPE_NOWAIT, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::StationsAndDesktops::{
    CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, DESKTOP_READOBJECTS,
    DESKTOP_READ_CONTROL, UOI_NAME,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::windows_appcontainer_support::wide;
use super::windows_desktop_session_native::{
    SpawnedDesktopHelper, WindowsDesktopSessionHost, WindowsDesktopSessionSnapshot,
};

const PIPE_BUFFER_BYTES: u32 = 64 * 1024;
const MAX_FRAME_BYTES: usize = 256 * 1024;
const IO_POLL: Duration = Duration::from_millis(10);
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const DESKTOP_PIPE_PREFIX: &str = r"\\.\pipe\forme-desktop-helper-v1-";

pub(crate) fn connect_server(
    server: OwnedHandle,
    expected_pid: u32,
    spawned: &SpawnedDesktopHelper,
    snapshot: &WindowsDesktopSessionSnapshot,
    host: &WindowsDesktopSessionHost,
) -> p::Result<File> {
    let deadline = Instant::now() + HELLO_TIMEOUT;
    loop {
        let connected =
            unsafe { ConnectNamedPipe(server.as_raw_handle() as HANDLE, null_mut()) } != 0;
        if connected || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED {
            break;
        }
        let error = unsafe { GetLastError() };
        if error != ERROR_PIPE_LISTENING {
            return Err(native_error("desktop helper pipe connection is unknown"));
        }
        if Instant::now() >= deadline {
            return Err(p::Error("desktop helper pipe connection timed out".into()));
        }
        std::thread::sleep(IO_POLL);
    }
    let mode = PIPE_READMODE_BYTE | PIPE_WAIT;
    if unsafe { SetNamedPipeHandleState(server.as_raw_handle() as HANDLE, &mode, null(), null()) }
        == 0
    {
        return Err(native_error(
            "desktop helper pipe could not enter bounded mode",
        ));
    }
    verify_client(
        server.as_raw_handle() as HANDLE,
        expected_pid,
        spawned,
        snapshot,
        host,
    )?;
    let raw_handle = server.into_raw_handle();
    // SAFETY: ownership is transferred exactly once after peer verification.
    Ok(unsafe { File::from_raw_handle(raw_handle) })
}

fn verify_client(
    pipe: HANDLE,
    expected_pid: u32,
    spawned: &SpawnedDesktopHelper,
    snapshot: &WindowsDesktopSessionSnapshot,
    host: &WindowsDesktopSessionHost,
) -> p::Result<()> {
    let mut pid = 0u32;
    if unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) } == 0 || pid != expected_pid {
        return Err(p::Error(
            "desktop helper peer PID is not the launched process".into(),
        ));
    }
    if spawned.pid != pid {
        return Err(p::Error(
            "desktop helper process identity is inconsistent".into(),
        ));
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    let process = owned_handle(process, "desktop helper process cannot be opened")?;
    let image = process_image_path(process.as_raw_handle() as HANDLE)?;
    let image = std::fs::canonicalize(image)
        .map_err(|_| p::Error("desktop helper image cannot be canonicalized".into()))?;
    let expected = host.helper_image();
    if !image
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.to_string_lossy())
    {
        return Err(p::Error(
            "desktop helper image is not the active release".into(),
        ));
    }
    let mut token = null_mut();
    if unsafe { OpenProcessToken(process.as_raw_handle() as HANDLE, TOKEN_QUERY, &mut token) } == 0
    {
        return Err(native_error("desktop helper token cannot be opened"));
    }
    let token = owned_handle(token, "desktop helper token handle is invalid")?;
    if token_user_sid(&token)? != snapshot.user_sid
        || token_session_id(&token)? != snapshot.session_id
        || token_authentication_id(&token)? != snapshot.session_generation
    {
        return Err(p::Error(
            "desktop helper token is not bound to the session".into(),
        ));
    }
    Ok(())
}

pub(crate) fn create_pipe(name: &str, user_sid: &str) -> p::Result<(OwnedHandle, PipeSecurity)> {
    if !valid_sid_text(user_sid) {
        return Err(p::Error("desktop helper user SID is invalid".into()));
    }
    let security = PipeSecurity::new(user_sid)?;
    let name_w = wide(name);
    let handle = unsafe {
        CreateNamedPipeW(
            name_w.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            0,
            &security.attributes,
        )
    };
    Ok((
        owned_handle(handle, "desktop helper named pipe could not be created")?,
        security,
    ))
}

pub(crate) fn open_helper_pipe(name: &str) -> p::Result<File> {
    let name_w = wide(name);
    if unsafe { WaitNamedPipeW(name_w.as_ptr(), 30_000) } == 0 {
        return Err(native_error("desktop helper named pipe is unavailable"));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .security_qos_flags(SECURITY_IDENTIFICATION)
        .open(name)
        .map_err(|_| p::Error("desktop helper named pipe could not be opened".into()))?;
    let mode = PIPE_READMODE_BYTE | PIPE_WAIT;
    if unsafe { SetNamedPipeHandleState(file.as_raw_handle() as HANDLE, &mode, null(), null()) }
        == 0
    {
        return Err(native_error("desktop helper pipe mode could not be set"));
    }
    Ok(file)
}

pub(crate) fn verify_interactive_desktop() -> p::Result<()> {
    let desktop = unsafe { OpenInputDesktop(0, 0, DESKTOP_READOBJECTS | DESKTOP_READ_CONTROL) };
    if desktop.is_null() || std::ptr::eq(desktop, INVALID_HANDLE_VALUE) {
        return Err(native_error(
            "desktop helper interactive desktop is unavailable",
        ));
    }
    let result = (|| {
        let mut bytes = 0u32;
        unsafe { GetUserObjectInformationW(desktop, UOI_NAME, null_mut(), 0, &mut bytes) };
        if bytes == 0 || bytes > 512 {
            return Err(p::Error(
                "desktop helper desktop name is unavailable".into(),
            ));
        }
        let mut buffer = vec![0u16; (bytes as usize).div_ceil(2).max(1)];
        if unsafe {
            GetUserObjectInformationW(
                desktop,
                UOI_NAME,
                buffer.as_mut_ptr() as *mut c_void,
                bytes,
                &mut bytes,
            )
        } == 0
        {
            return Err(native_error("desktop helper desktop name cannot be read"));
        }
        let end = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        let name = String::from_utf16(&buffer[..end])
            .map_err(|_| p::Error("desktop helper desktop name is invalid".into()))?;
        if name != "Default" {
            return Err(p::Error(
                "desktop helper is not on the interactive desktop".into(),
            ));
        }
        Ok(())
    })();
    unsafe { CloseDesktop(desktop) };
    result
}

pub(crate) fn desktop_pipe_name(binding: &p::DesktopSessionBinding) -> p::Result<String> {
    let digest = p::canonical_digest(&(
        "forme-desktop-helper-pipe-v1",
        &binding.helper_ref,
        &binding.grant_ref,
        &binding.nonce,
        binding.session_generation,
    ))?
    .0;
    Ok(format!("{DESKTOP_PIPE_PREFIX}{digest}"))
}

pub(crate) fn helper_pipe_argument() -> p::Result<String> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.len() != 3
        || arguments[0] != "--desktop-session-helper"
        || arguments[1] != "--pipe"
    {
        return Err(p::Error("desktop helper arguments are invalid".into()));
    }
    let pipe = arguments[2].clone();
    if !pipe.starts_with(DESKTOP_PIPE_PREFIX)
        || pipe.len() > 256
        || pipe.chars().any(char::is_control)
    {
        return Err(p::Error("desktop helper pipe is invalid".into()));
    }
    Ok(pipe)
}

const fn valid_frame_length(length: usize) -> bool {
    length > 0 && length <= MAX_FRAME_BYTES
}

pub(crate) fn write_frame<T: Serialize>(stream: &mut File, value: &T) -> p::Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| p::Error("desktop helper frame cannot be encoded".into()))?;
    if !valid_frame_length(bytes.len()) {
        return Err(p::Error("desktop helper frame is outside its bound".into()));
    }
    let length = u32::try_from(bytes.len())
        .map_err(|_| p::Error("desktop helper frame is too large".into()))?;
    stream
        .write_all(&length.to_le_bytes())
        .map_err(|_| p::Error("desktop helper frame cannot be written".into()))?;
    stream
        .write_all(&bytes)
        .map_err(|_| p::Error("desktop helper frame cannot be written".into()))?;
    stream
        .flush()
        .map_err(|_| p::Error("desktop helper frame cannot be flushed".into()))
}

pub(crate) fn read_frame<T: DeserializeOwned>(
    stream: &mut File,
    deadline: Instant,
) -> p::Result<T> {
    let mut length = [0u8; 4];
    read_bounded(stream, &mut length, deadline)?;
    let length = u32::from_le_bytes(length) as usize;
    if !valid_frame_length(length) {
        return Err(p::Error("desktop helper frame length is invalid".into()));
    }
    let mut bytes = vec![0u8; length];
    read_bounded(stream, &mut bytes, deadline)?;
    serde_json::from_slice(&bytes).map_err(|_| p::Error("desktop helper frame is malformed".into()))
}

fn read_bounded(stream: &mut File, output: &mut [u8], deadline: Instant) -> p::Result<()> {
    let mut offset = 0usize;
    while offset < output.len() {
        if Instant::now() >= deadline {
            return Err(p::Error("desktop helper I/O deadline expired".into()));
        }
        let available = pipe_available(stream)?;
        if available == 0 {
            std::thread::sleep(IO_POLL);
            continue;
        }
        let amount = (available as usize).min(output.len() - offset);
        stream
            .read_exact(&mut output[offset..offset + amount])
            .map_err(|_| p::Error("desktop helper peer disconnected".into()))?;
        offset += amount;
    }
    Ok(())
}

fn pipe_available(stream: &File) -> p::Result<u32> {
    let mut available = 0u32;
    if unsafe {
        PeekNamedPipe(
            stream.as_raw_handle() as HANDLE,
            null_mut(),
            0,
            null_mut(),
            &mut available,
            null_mut(),
        )
    } != 0
    {
        return Ok(available);
    }
    let error = unsafe { GetLastError() };
    if matches!(error, ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED) {
        Err(p::Error("desktop helper peer disconnected".into()))
    } else {
        Err(native_error("desktop helper pipe availability is unknown"))
    }
}

fn token_user_sid(token: &OwnedHandle) -> p::Result<String> {
    let bytes = token_information(token, TokenUser)?;
    if bytes.len() < size_of::<TOKEN_USER>() {
        return Err(p::Error(
            "desktop helper token user information is truncated".into(),
        ));
    }
    let user = unsafe { &*(bytes.as_ptr() as *const TOKEN_USER) };
    sid_string(user.User.Sid)
}

fn token_authentication_id(token: &OwnedHandle) -> p::Result<u64> {
    let bytes = token_information(token, TokenStatistics)?;
    if bytes.len() < size_of::<TOKEN_STATISTICS>() {
        return Err(p::Error(
            "desktop helper token statistics are truncated".into(),
        ));
    }
    let value = unsafe { &*(bytes.as_ptr() as *const TOKEN_STATISTICS) }.AuthenticationId;
    Ok(((value.HighPart as u32 as u64) << 32) | u64::from(value.LowPart))
}

fn token_session_id(token: &OwnedHandle) -> p::Result<u32> {
    let bytes = token_information(token, TokenSessionId)?;
    if bytes.len() < size_of::<u32>() {
        return Err(p::Error(
            "desktop helper token session information is truncated".into(),
        ));
    }
    Ok(unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const u32) })
}

fn token_information(token: &OwnedHandle, class: i32) -> p::Result<Vec<u8>> {
    let mut bytes = 0u32;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle() as HANDLE,
            class,
            null_mut(),
            0,
            &mut bytes,
        )
    };
    if bytes == 0 || bytes > 1 << 20 {
        return Err(native_error(
            "desktop helper token information size is unavailable",
        ));
    }
    let mut output = vec![0u8; bytes as usize];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle() as HANDLE,
            class,
            output.as_mut_ptr() as *mut c_void,
            bytes,
            &mut bytes,
        )
    } == 0
    {
        return Err(native_error(
            "desktop helper token information cannot be read",
        ));
    }
    output.truncate(bytes as usize);
    Ok(output)
}

fn sid_string(sid: windows_sys::Win32::Security::PSID) -> p::Result<String> {
    let mut text = null_mut();
    if sid.is_null() || unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
        return Err(native_error("desktop helper token SID cannot be converted"));
    }
    let result = (|| {
        let mut length = 0usize;
        while length < 184 && unsafe { *text.add(length) } != 0 {
            length += 1;
        }
        if length == 184 {
            return Err(p::Error("desktop helper SID string is unbounded".into()));
        }
        String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) })
            .map_err(|_| p::Error("desktop helper SID string is invalid".into()))
    })();
    unsafe { windows_sys::Win32::Foundation::LocalFree(text as HLOCAL) };
    result
}

fn process_image_path(process: HANDLE) -> p::Result<std::path::PathBuf> {
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
    {
        return Err(native_error("desktop helper process image is unavailable"));
    }
    String::from_utf16(&buffer[..length as usize])
        .map(std::path::PathBuf::from)
        .map_err(|_| p::Error("desktop helper process image is invalid".into()))
}

pub(crate) struct PipeSecurity {
    descriptor: *mut c_void,
    attributes: SECURITY_ATTRIBUTES,
}

impl PipeSecurity {
    fn new(user_sid: &str) -> p::Result<Self> {
        let sddl = wide(&format!("D:P(A;;GA;;;SY)(A;;GA;;;{user_sid})"));
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
                "desktop helper pipe security descriptor cannot be created",
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
            unsafe { windows_sys::Win32::Foundation::LocalFree(self.descriptor as HLOCAL) };
        }
    }
}

fn valid_sid_text(value: &str) -> bool {
    value.starts_with("S-1-")
        && value.len() <= 184
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-' || byte == b'S')
}

fn owned_handle(handle: HANDLE, message: &'static str) -> p::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(native_error(message));
    }
    // SAFETY: only a successful, uniquely owned Win32 handle reaches this point.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
}

pub(crate) fn now_ms() -> p::Result<p::Timestamp> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| p::Error("desktop helper clock is unavailable".into()))
        .and_then(|value| {
            i64::try_from(value.as_millis())
                .map_err(|_| p::Error("desktop helper clock overflowed".into()))
        })
}

pub(crate) fn timestamp_deadline(timestamp: p::Timestamp) -> Instant {
    let now = now_ms().unwrap_or(timestamp);
    let remaining = timestamp.saturating_sub(now).max(1) as u64;
    Instant::now() + Duration::from_millis(remaining.min(30_000))
}

fn native_error(message: &str) -> p::Error {
    let _ = unsafe { GetLastError() };
    p::Error(message.into())
}

#[cfg(test)]
mod tests {
    use super::valid_sid_text;

    #[test]
    fn pipe_acl_only_accepts_sid_shaped_values() {
        assert!(valid_sid_text("S-1-5-18"));
        assert!(!valid_sid_text("owner:one"));
        assert!(!valid_sid_text("S-1-5-18;D:(A;;GA;;;WD)"));
    }
}
