#![cfg(windows)]
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use forme_protocol as p;
use windows_sys::Win32::Foundation::{
    GetLastError, LocalFree, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LUID,
};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenSessionId, TokenStatistics, TokenUser, TOKEN_STATISTICS, TOKEN_USER,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::RemoteDesktop::{
    WTSActive, WTSConnectState, WTSEnumerateSessionsW, WTSFreeMemory, WTSQuerySessionInformationW,
    WTSQueryUserToken, WTSSessionInfoEx, WTSINFOEXW, WTS_CURRENT_SERVER_HANDLE,
    WTS_SESSIONSTATE_UNLOCK,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessAsUserW, ResumeThread, TerminateProcess, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
};

use super::windows_appcontainer_support::{quote_windows_arg, wide};

const DEFAULT_DESKTOP: &str = "desktop:windows-default";
const SESSION_PREFIX: &str = "os-session:";
const USER_PREFIX: &str = "os-user:";
const MAX_TOKEN_INFO_BYTES: usize = 1 << 20;

/// Safe Rust handle for the Host's fixed helper image and its process launch
/// boundary. The token and process handles never leave this module.
pub(crate) struct WindowsDesktopSessionHost {
    helper_image: PathBuf,
    output_release: Option<super::desktop_output_release::DesktopOutputRelease>,
}

pub(crate) struct WindowsDesktopSessionSnapshot {
    pub(crate) session_id: u32,
    pub(crate) os_user_ref: String,
    pub(crate) os_session_ref: String,
    pub(crate) session_generation: u64,
    pub(crate) desktop_ref: String,
    pub(crate) user_sid: String,
    token: OwnedHandle,
}

pub(crate) struct SpawnedDesktopHelper {
    pub(crate) process: OwnedHandle,
    pub(crate) job: OwnedHandle,
    pub(crate) pid: u32,
}

impl WindowsDesktopSessionHost {
    pub(crate) fn new(helper_image: impl Into<PathBuf>) -> p::Result<Self> {
        let helper_image = helper_image.into();
        super::reject_linked_ancestors(&helper_image)?;
        if !helper_image.is_absolute() || !helper_image.is_file() {
            return Err(p::Error(
                "desktop helper image is not an installed regular file".into(),
            ));
        }
        let helper_image = helper_image
            .canonicalize()
            .map_err(|_| p::Error("desktop helper image cannot be canonicalized".into()))?;
        super::reject_linked_ancestors(&helper_image)?;
        Ok(Self {
            helper_image,
            output_release: None,
        })
    }

    pub(crate) fn from_release(release: &super::VerifiedStagedRelease) -> p::Result<Self> {
        let mut value = Self::new(release.content_root().join("bin/forme-executord.exe"))?;
        value.output_release =
            super::desktop_output_release::DesktopOutputRelease::from_release(release)?;
        if value
            .output_release
            .as_ref()
            .is_some_and(|output| output.image() != value.helper_image)
        {
            return Err(p::Error("desktop output image binding changed".into()));
        }
        Ok(value)
    }
    pub(crate) fn supports_output(&self) -> bool {
        self.output_release.is_some()
    }

    pub(crate) fn inspect(
        &self,
        binding: &p::DesktopSessionBinding,
    ) -> p::Result<WindowsDesktopSessionSnapshot> {
        if binding.platform != p::DesktopPlatform::Windows {
            return Err(p::Error(
                "Windows desktop session received another platform".into(),
            ));
        }
        let session_id = parse_session_id(&binding.os_session_ref)?;
        if binding.desktop_ref != DEFAULT_DESKTOP {
            return Err(p::Error(
                "desktop grant does not select the fixed interactive desktop".into(),
            ));
        }
        let snapshot = self.inspect_session(session_id)?;
        if binding.os_user_ref != snapshot.os_user_ref
            || binding.session_generation != snapshot.session_generation
        {
            return Err(p::Error(
                "desktop grant does not match the live user and session generation".into(),
            ));
        }
        Ok(snapshot)
    }

    fn inspect_session(&self, session_id: u32) -> p::Result<WindowsDesktopSessionSnapshot> {
        let state = query_u32(session_id, WTSConnectState)? as i32;
        if state != WTSActive {
            return Err(p::Error("Windows desktop session is not active".into()));
        }
        let info = query_session_info_ex(session_id)?;
        if info.session_state != WTSActive || info.session_flags != WTS_SESSIONSTATE_UNLOCK as i32 {
            return Err(p::Error(
                "Windows desktop session is locked or unavailable".into(),
            ));
        }

        let token = query_user_token(session_id)?;
        let user_sid = token_user_sid(&token)?;
        let os_user_ref = format!("{USER_PREFIX}{user_sid}");
        let session_token_id = token_session_id(&token)?;
        if session_token_id != session_id {
            return Err(p::Error("desktop token belongs to another session".into()));
        }
        let session_generation = token_authentication_id(&token)?;
        if session_generation == 0 {
            return Err(p::Error(
                "desktop grant session generation does not match the live token".into(),
            ));
        }
        Ok(WindowsDesktopSessionSnapshot {
            session_id,
            os_user_ref,
            os_session_ref: format!("{SESSION_PREFIX}{session_id}"),
            session_generation,
            desktop_ref: DEFAULT_DESKTOP.into(),
            user_sid,
            token,
        })
    }

    pub(crate) fn catalog(&self) -> p::Result<Vec<super::ObservedDesktopSession>> {
        let mut sessions = Vec::new();
        for id in active_session_ids()? {
            // Locked, disconnected, or unqueryable token sessions confer no
            // choice authority. Rechecking a selected session never skips errors.
            if let Ok(snapshot) = self.inspect_session(id) {
                sessions.push(observed(&snapshot, self.supports_output()));
            }
        }
        Ok(sessions)
    }

    pub(crate) fn recheck_observed(
        &self,
        original: &super::ObservedDesktopSession,
    ) -> p::Result<()> {
        let snapshot = self.inspect_session(parse_session_id(&original.os_session_ref)?)?;
        if observed(&snapshot, self.supports_output()) != *original {
            return Err(p::Error(
                "desktop catalog session changed before admission".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn helper_image(&self) -> &Path {
        &self.helper_image
    }

    pub(crate) fn spawn_helper(
        &self,
        snapshot: &WindowsDesktopSessionSnapshot,
        pipe_name: &str,
    ) -> p::Result<SpawnedDesktopHelper> {
        self.spawn_mode(snapshot, pipe_name, false)
    }
    pub(crate) fn spawn_output_helper(
        &self,
        snapshot: &WindowsDesktopSessionSnapshot,
        pipe_name: &str,
    ) -> p::Result<SpawnedDesktopHelper> {
        if !self.supports_output() {
            return Err(p::Error("desktop output release is unavailable".into()));
        }
        self.spawn_mode(snapshot, pipe_name, true)
    }
    fn spawn_mode(
        &self,
        snapshot: &WindowsDesktopSessionSnapshot,
        pipe_name: &str,
        output: bool,
    ) -> p::Result<SpawnedDesktopHelper> {
        if !pipe_name.starts_with(r"\\.\pipe\forme-desktop-helper-v1-")
            || pipe_name.len() > 256
            || pipe_name.chars().any(char::is_control)
        {
            return Err(p::Error("desktop helper pipe name is invalid".into()));
        }
        let image = self
            .helper_image
            .to_str()
            .ok_or_else(|| p::Error("desktop helper image path is not Unicode".into()))?;
        let command = format!(
            "{} {} --pipe {}",
            quote_windows_arg(image),
            if output {
                "--desktop-session-helper-output-v2"
            } else {
                "--desktop-session-helper"
            },
            quote_windows_arg(pipe_name)
        );
        let application = wide(image);
        let mut command_line = wide(&command);
        let mut desktop = wide("winsta0\\default");
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            lpDesktop: desktop.as_mut_ptr(),
            ..STARTUPINFOW::default()
        };
        let mut process_info = PROCESS_INFORMATION::default();
        let job = create_kill_on_close_job()?;
        let created = unsafe {
            CreateProcessAsUserW(
                snapshot.token.as_raw_handle() as HANDLE,
                application.as_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                0,
                CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
                null(),
                null(),
                &startup,
                &mut process_info,
            )
        };
        if created == 0 {
            return Err(native_error("desktop helper process could not be created"));
        }
        let process = match owned_handle(
            process_info.hProcess,
            "desktop helper process handle is invalid",
        ) {
            Ok(handle) => handle,
            Err(error) => {
                unsafe { TerminateProcess(process_info.hProcess, 1) };
                return Err(error);
            }
        };
        let thread = match owned_handle(
            process_info.hThread,
            "desktop helper thread handle is invalid",
        ) {
            Ok(handle) => handle,
            Err(error) => {
                unsafe { TerminateProcess(process.as_raw_handle() as HANDLE, 1) };
                return Err(error);
            }
        };
        if unsafe {
            AssignProcessToJobObject(
                job.as_raw_handle() as HANDLE,
                process.as_raw_handle() as HANDLE,
            )
        } == 0
        {
            unsafe { TerminateProcess(process.as_raw_handle() as HANDLE, 1) };
            return Err(native_error(
                "desktop helper could not enter its kill-on-close job",
            ));
        }
        if unsafe { ResumeThread(thread.as_raw_handle() as HANDLE) } == u32::MAX {
            unsafe { TerminateProcess(process.as_raw_handle() as HANDLE, 1) };
            return Err(native_error("desktop helper thread could not be resumed"));
        }
        drop(thread);
        Ok(SpawnedDesktopHelper {
            process,
            job,
            pid: process_info.dwProcessId,
        })
    }
}

fn create_kill_on_close_job() -> p::Result<OwnedHandle> {
    let job = unsafe { CreateJobObjectW(null(), null()) };
    let job = owned_handle(job, "desktop helper job could not be created")?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle() as HANDLE,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const c_void,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(native_error("desktop helper job limits could not be set"));
    }
    Ok(job)
}

fn query_user_token(session_id: u32) -> p::Result<OwnedHandle> {
    let mut token = null_mut();
    if unsafe { WTSQueryUserToken(session_id, &mut token) } == 0 {
        return Err(native_error("Windows user session token is unavailable"));
    }
    owned_handle(token, "Windows user session token handle is invalid")
}

fn query_u32(session_id: u32, class: i32) -> p::Result<u32> {
    with_wts_buffer(session_id, class, |buffer, bytes| {
        if bytes < size_of::<u32>() as u32 {
            return Err(p::Error("Windows session information is truncated".into()));
        }
        Ok(unsafe { std::ptr::read_unaligned(buffer as *const u32) })
    })
}

struct SessionInfoEx {
    session_state: i32,
    session_flags: i32,
}

fn query_session_info_ex(session_id: u32) -> p::Result<SessionInfoEx> {
    with_wts_buffer(session_id, WTSSessionInfoEx, |buffer, bytes| {
        if bytes < size_of::<WTSINFOEXW>() as u32 {
            return Err(p::Error(
                "Windows session state information is truncated".into(),
            ));
        }
        let info = unsafe { &*(buffer as *const WTSINFOEXW) };
        if info.Level != 1 {
            return Err(p::Error(
                "Windows session state information has an unknown level".into(),
            ));
        }
        let level = unsafe { info.Data.WTSInfoExLevel1 };
        Ok(SessionInfoEx {
            session_state: level.SessionState,
            session_flags: level.SessionFlags,
        })
    })
}

fn with_wts_buffer<T>(
    session_id: u32,
    class: i32,
    read: impl FnOnce(*mut u16, u32) -> p::Result<T>,
) -> p::Result<T> {
    let mut buffer = null_mut();
    let mut bytes = 0u32;
    if unsafe {
        WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            session_id,
            class,
            &mut buffer,
            &mut bytes,
        )
    } == 0
        || buffer.is_null()
    {
        return Err(native_error("Windows session information is unavailable"));
    }
    let result = if bytes as usize > MAX_TOKEN_INFO_BYTES {
        Err(p::Error("Windows session information is unbounded".into()))
    } else {
        read(buffer, bytes)
    };
    unsafe { WTSFreeMemory(buffer as *mut c_void) };
    result
}

fn token_user_sid(token: &OwnedHandle) -> p::Result<String> {
    let bytes = token_information(token, TokenUser)?;
    if bytes.len() < size_of::<TOKEN_USER>() {
        return Err(p::Error(
            "Windows token user information is truncated".into(),
        ));
    }
    let user = unsafe { &*(bytes.as_ptr() as *const TOKEN_USER) };
    sid_string(user.User.Sid)
}

fn token_authentication_id(token: &OwnedHandle) -> p::Result<u64> {
    let bytes = token_information(token, TokenStatistics)?;
    if bytes.len() < size_of::<TOKEN_STATISTICS>() {
        return Err(p::Error("Windows token statistics are truncated".into()));
    }
    let statistics = unsafe { &*(bytes.as_ptr() as *const TOKEN_STATISTICS) };
    luid_value(statistics.AuthenticationId)
}

fn token_session_id(token: &OwnedHandle) -> p::Result<u32> {
    let bytes = token_information(token, TokenSessionId)?;
    if bytes.len() < size_of::<u32>() {
        return Err(p::Error(
            "Windows token session information is truncated".into(),
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
        );
    }
    if bytes == 0 || bytes as usize > MAX_TOKEN_INFO_BYTES {
        return Err(native_error(
            "Windows token information size is unavailable",
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
        return Err(native_error("Windows token information cannot be read"));
    }
    output.truncate(bytes as usize);
    Ok(output)
}

fn sid_string(sid: windows_sys::Win32::Security::PSID) -> p::Result<String> {
    if sid.is_null() {
        return Err(p::Error("Windows token SID is null".into()));
    }
    let mut text = null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
        return Err(native_error("Windows token SID cannot be converted"));
    }
    let result = (|| {
        let mut length = 0usize;
        while length < 184 && unsafe { *text.add(length) } != 0 {
            length += 1;
        }
        if length == 184 {
            return Err(p::Error("Windows token SID string is unbounded".into()));
        }
        String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) })
            .map_err(|_| p::Error("Windows token SID string is invalid".into()))
    })();
    unsafe { LocalFree(text as HLOCAL) };
    result
}

fn luid_value(value: LUID) -> p::Result<u64> {
    Ok(((value.HighPart as u32 as u64) << 32) | u64::from(value.LowPart))
}

fn parse_session_id(value: &str) -> p::Result<u32> {
    let id = value
        .strip_prefix(SESSION_PREFIX)
        .ok_or_else(|| p::Error("desktop OS session reference is invalid".into()))?
        .parse::<u32>()
        .map_err(|_| p::Error("desktop OS session reference is invalid".into()))?;
    if id > 0xffff {
        return Err(p::Error(
            "desktop OS session identifier is out of range".into(),
        ));
    }
    Ok(id)
}

fn owned_handle(handle: HANDLE, message: &'static str) -> p::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(native_error(message));
    }
    // SAFETY: only a successful, uniquely owned Win32 handle reaches this point.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
}

fn observed(
    snapshot: &WindowsDesktopSessionSnapshot,
    output: bool,
) -> super::ObservedDesktopSession {
    super::ObservedDesktopSession {
        schema_version: p::SchemaVersion(1),
        helper_ref: "helper:installed-desktop".into(),
        platform: p::DesktopPlatform::Windows,
        label: "Windows 桌面会话".into(),
        os_user_ref: snapshot.os_user_ref.clone(),
        os_session_ref: snapshot.os_session_ref.clone(),
        session_generation: snapshot.session_generation,
        desktop_ref: snapshot.desktop_ref.clone(),
        capabilities: [
            p::DesktopCapability::PointerMove,
            p::DesktopCapability::PointerClick,
            p::DesktopCapability::KeyboardType,
            p::DesktopCapability::KeyboardKey,
            p::DesktopCapability::Scroll,
        ]
        .into_iter()
        .chain(output.then_some(p::DesktopCapability::Screenshot))
        .collect(),
    }
}
fn active_session_ids() -> p::Result<Vec<u32>> {
    let mut sessions = null_mut();
    let mut count = 0;
    let succeeded = unsafe {
        WTSEnumerateSessionsW(WTS_CURRENT_SERVER_HANDLE, 0, 1, &mut sessions, &mut count)
    } != 0;
    let result = if !succeeded
        || count as usize > p::DESKTOP_CATALOG_MAX_ENTRIES
        || (count > 0 && sessions.is_null())
    {
        Err(p::Error(
            "Windows desktop session catalog is unavailable or exceeds its bound".into(),
        ))
    } else if count == 0 {
        Ok(Vec::new())
    } else {
        // SAFETY: WTS owns this count-sized array until WTSFreeMemory below;
        // count is bounded and only scalar session identifiers leave this scope.
        let entries = unsafe { std::slice::from_raw_parts(sessions, count as usize) };
        Ok(entries
            .iter()
            .filter(|entry| entry.State == WTSActive)
            .map(|entry| entry.SessionId)
            .collect())
    };
    if !sessions.is_null() {
        unsafe { WTSFreeMemory(sessions.cast()) };
    }
    result
}

fn native_error(message: &str) -> p::Error {
    let _ = unsafe { GetLastError() };
    p::Error(message.into())
}

#[cfg(test)]
mod tests {
    use super::{parse_session_id, DEFAULT_DESKTOP};

    #[test]
    fn session_reference_is_bounded() {
        assert_eq!(parse_session_id("os-session:7").unwrap(), 7);
        assert!(parse_session_id("session:7").is_err());
        assert!(parse_session_id("os-session:65536").is_err());
    }

    #[test]
    fn desktop_reference_is_fixed() {
        assert_eq!(DEFAULT_DESKTOP, "desktop:windows-default");
        forme_protocol::validate_product_id(DEFAULT_DESKTOP).unwrap();
    }
}
