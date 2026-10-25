#![cfg(windows)]
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::mem::{size_of, size_of_val};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use forme_protocol as p;
use windows_sys::Win32::Foundation::{
    GetLastError, SetLastError, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND,
    ERROR_INSUFFICIENT_BUFFER, HANDLE, INVALID_HANDLE_VALUE, LUID, WAIT_FAILED, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::{
    AdjustTokenPrivileges, EqualSid, GetSidIdentifierAuthority, GetSidSubAuthority,
    GetSidSubAuthorityCount, GetTokenInformation, LookupPrivilegeValueW, TokenAppContainerSid,
    TokenCapabilities, TokenIntegrityLevel, TokenIsAppContainer, TokenPrivileges,
    LUID_AND_ATTRIBUTES, SECURITY_CAPABILITIES, SECURITY_MANDATORY_LABEL_AUTHORITY,
    SE_CHANGE_NOTIFY_NAME, SE_PRIVILEGE_REMOVED, SID_AND_ATTRIBUTES, TOKEN_ADJUST_PRIVILEGES,
    TOKEN_MANDATORY_LABEL, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    OpenJobObjectW, QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::SystemServices::{
    JOB_OBJECT_QUERY, JOB_OBJECT_TERMINATE, SECURITY_MANDATORY_LOW_RID,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, OpenProcessToken, ResumeThread, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    DETACHED_PROCESS, EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTUPINFOEXW,
};

use super::windows_appcontainer_acl::Profile;
use super::windows_appcontainer_support::{
    environment_block, quote_windows_arg, wide, WorkerEnvironment,
};

mod runtime;
pub(crate) use runtime::{CancelState, LaunchRequest, NativeWorker, SetupResources};

const CANCEL_WAIT: Duration = Duration::from_secs(30);
const STILL_ACTIVE: u32 = 259;

fn create_job(name: &str) -> p::Result<OwnedHandle> {
    let name = wide(name);
    unsafe { SetLastError(0) };
    let job = unsafe { CreateJobObjectW(null(), name.as_ptr()) };
    let last_error = unsafe { GetLastError() };
    let job = owned_handle(job, "Job Object could not be created")?;
    if last_error == ERROR_ALREADY_EXISTS {
        return Err(p::Error(
            "worker Job Object identity already exists and requires recovery".into(),
        ));
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let ok = unsafe {
        SetInformationJobObject(
            raw(&job),
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const c_void,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if ok == 0 {
        return Err(native_error(
            "Job Object kill-on-close could not be enabled",
        ));
    }
    Ok(job)
}

pub(crate) fn recover_named_job(name: &str) -> p::Result<()> {
    let name = wide(name);
    let job = unsafe { OpenJobObjectW(JOB_OBJECT_QUERY | JOB_OBJECT_TERMINATE, 0, name.as_ptr()) };
    if job.is_null() {
        if unsafe { GetLastError() } == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        return Err(native_error(
            "the original worker Job Object cannot be opened for recovery",
        ));
    }
    let job = owned_handle(job, "the original worker Job Object handle is invalid")?;
    if active_processes(raw(&job))? != 0 {
        if unsafe { TerminateJobObject(raw(&job), 1) } == 0 && active_processes(raw(&job))? != 0 {
            return Err(native_error(
                "the original worker Job Object could not be terminated",
            ));
        }
        wait_for_job_empty(raw(&job))?;
    }
    if active_processes(raw(&job))? != 0 {
        return Err(p::Error(
            "the original worker Job Object still contains active processes".into(),
        ));
    }
    Ok(())
}

pub(super) fn resume_worker_thread(thread: HANDLE) -> p::Result<()> {
    if unsafe { ResumeThread(thread) } != 1 {
        return Err(native_error(
            "worker thread could not be resumed after verification",
        ));
    }
    Ok(())
}

pub(super) fn terminate_job_if_active(job: HANDLE) -> p::Result<()> {
    if active_processes(job)? == 0 {
        return Ok(());
    }
    if unsafe { TerminateJobObject(job, 1) } == 0 && active_processes(job)? != 0 {
        return Err(native_error("worker Job Object could not be terminated"));
    }
    wait_for_job_empty(job)
}

pub(super) fn terminate_process_if_active(process: HANDLE) -> p::Result<()> {
    if !process_exited(process)?
        && unsafe { TerminateProcess(process, 1) } == 0
        && !process_exited(process)?
    {
        return Err(native_error("worker setup process could not be terminated"));
    }
    wait_for_process(process)
}

fn launch_suspended(
    request: &LaunchRequest<'_>,
    profile: &Profile,
    job: HANDLE,
) -> p::Result<(OwnedHandle, OwnedHandle)> {
    let program = request
        .spec
        .program
        .to_str()
        .ok_or_else(|| p::Error("worker program path is not valid Unicode".into()))?;
    let current_dir = request
        .workspace
        .to_str()
        .ok_or_else(|| p::Error("worker workspace path is not valid Unicode".into()))?;
    let mut command = quote_windows_arg(program);
    for arg in &request.spec.args {
        command.push(' ');
        command.push_str(&quote_windows_arg(arg));
    }
    let mut command = wide(&command);
    let application = wide(program);
    let directory = wide(current_dir);
    let environment = environment_block(
        WorkerEnvironment {
            job_path: request.job_path,
            permit_path: request.permit_path,
            workspace: request.workspace,
            execution_path: request.execution_path,
            workspace_scope: request.workspace_scope,
            deadline: request.deadline,
        },
        &system_windows_directory()?,
        &super::windows_appcontainer_acl::local_app_data()?,
    )?;
    let mut capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: profile.raw_sid(),
        Capabilities: null_mut::<SID_AND_ATTRIBUTES>(),
        CapabilityCount: 0,
        Reserved: 0,
    };
    let mut attribute_bytes = 0usize;
    unsafe {
        InitializeProcThreadAttributeList(null_mut(), 3, 0, &mut attribute_bytes);
    }
    if attribute_bytes == 0 {
        return Err(native_error(
            "AppContainer process attribute size is unavailable",
        ));
    }
    let word_count = attribute_bytes.div_ceil(size_of::<usize>());
    let mut attribute_storage = vec![0usize; word_count];
    let attributes = attribute_storage.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
    if unsafe { InitializeProcThreadAttributeList(attributes, 3, 0, &mut attribute_bytes) } == 0 {
        return Err(native_error(
            "AppContainer process attributes could not be initialized",
        ));
    }
    let update = unsafe {
        UpdateProcThreadAttribute(
            attributes,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            &mut capabilities as *mut _ as *const c_void,
            size_of::<SECURITY_CAPABILITIES>(),
            null_mut(),
            null(),
        )
    };
    if update == 0 {
        unsafe { DeleteProcThreadAttributeList(attributes) };
        return Err(native_error(
            "AppContainer security capabilities could not be attached",
        ));
    }
    let job_list = [job];
    let update = unsafe {
        UpdateProcThreadAttribute(
            attributes,
            0,
            PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
            job_list.as_ptr() as *const c_void,
            size_of_val(&job_list),
            null_mut(),
            null(),
        )
    };
    if update == 0 {
        unsafe { DeleteProcThreadAttributeList(attributes) };
        return Err(native_error(
            "worker could not be atomically assigned to its Job Object",
        ));
    }
    // PROCESS_CREATION_CHILD_PROCESS_RESTRICTED. A file creator can choose its
    // own DACL, so an inheritable execute-deny ACE alone is not an execution
    // boundary for a writable copy. Block child creation in the OS instead.
    let child_policy: u32 = 1;
    if unsafe {
        UpdateProcThreadAttribute(
            attributes,
            0,
            PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY as usize,
            &child_policy as *const _ as *const c_void,
            size_of_val(&child_policy),
            null_mut(),
            null(),
        )
    } == 0
    {
        unsafe { DeleteProcThreadAttributeList(attributes) };
        return Err(native_error(
            "worker child-process restriction could not be applied",
        ));
    }
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attributes;
    let mut info = PROCESS_INFORMATION::default();
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            0,
            CREATE_SUSPENDED
                | EXTENDED_STARTUPINFO_PRESENT
                | CREATE_UNICODE_ENVIRONMENT
                | DETACHED_PROCESS,
            environment.as_ptr() as *const c_void,
            directory.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        )
    };
    let creation_error = unsafe { GetLastError() };
    unsafe { DeleteProcThreadAttributeList(attributes) };
    if created == 0 {
        return Err(p::Error(format!(
            "AppContainer worker process could not be created suspended (Win32 {creation_error})"
        )));
    }
    let process = owned_handle(info.hProcess, "worker process handle is invalid")?;
    let thread = match owned_handle(info.hThread, "worker thread handle is invalid") {
        Ok(thread) => thread,
        Err(error) => {
            unsafe { TerminateProcess(raw(&process), 1) };
            let _ = unsafe { WaitForSingleObject(raw(&process), 30_000) };
            return Err(error);
        }
    };
    Ok((process, thread))
}

fn system_windows_directory() -> p::Result<std::path::PathBuf> {
    use windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
    let mut buffer = vec![0u16; 32_768];
    let length =
        unsafe { GetSystemWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(native_error("Windows runtime directory is unavailable"));
    }
    let text = String::from_utf16(&buffer[..length])
        .map_err(|_| native_error("Windows runtime path is invalid"))?;
    let path = std::path::PathBuf::from(text);
    if !path.is_absolute() {
        return Err(native_error("Windows runtime path is not absolute"));
    }
    Ok(path)
}

fn verify_worker_token(
    process: HANDLE,
    expected_sid: windows_sys::Win32::Security::PSID,
) -> p::Result<()> {
    let mut token = null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES, &mut token) } == 0
    {
        return Err(native_error(
            "worker token could not be opened for verification",
        ));
    }
    let token = owned_handle(token, "worker token handle is invalid")?;
    let is_app = token_information(&token, TokenIsAppContainer)?;
    if read_struct::<i32>(&is_app)? == 0 {
        return Err(p::Error("worker token is not an AppContainer token".into()));
    }
    let app_sid = token_information(&token, TokenAppContainerSid)?;
    let actual_sid = read_struct::<windows_sys::Win32::Security::PSID>(&app_sid)?;
    if actual_sid.is_null() || unsafe { EqualSid(actual_sid, expected_sid) } == 0 {
        return Err(p::Error(
            "worker token AppContainer SID does not match the job".into(),
        ));
    }
    let integrity = token_information(&token, TokenIntegrityLevel)?;
    let label = read_struct::<TOKEN_MANDATORY_LABEL>(&integrity)?;
    if label.Label.Sid.is_null() {
        return Err(p::Error("worker token integrity SID is missing".into()));
    }
    let count = unsafe { GetSidSubAuthorityCount(label.Label.Sid) };
    let authority = unsafe { GetSidIdentifierAuthority(label.Label.Sid) };
    if count.is_null() || authority.is_null() || unsafe { *count } == 0 {
        return Err(p::Error("worker token integrity SID is invalid".into()));
    }
    let subauthority_count = unsafe { *count };
    let rid = unsafe { GetSidSubAuthority(label.Label.Sid, (*count - 1) as u32) };
    if rid.is_null()
        || !is_low_integrity_sid(unsafe { (*authority).Value }, subauthority_count, unsafe {
            *rid
        })
    {
        return Err(p::Error("worker token is not low integrity".into()));
    }
    let capabilities = token_information(&token, TokenCapabilities)?;
    // TOKEN_GROUPS has a variable tail; a zero count does not require Groups[0].
    if read_struct::<u32>(&capabilities)? != 0 {
        return Err(p::Error(
            "worker token contains unexpected capabilities".into(),
        ));
    }
    verify_worker_privileges(&token)?;
    Ok(())
}

fn is_low_integrity_sid(authority: [u8; 6], subauthority_count: u8, rid: u32) -> bool {
    authority == SECURITY_MANDATORY_LABEL_AUTHORITY.Value
        && subauthority_count == 1
        && rid == SECURITY_MANDATORY_LOW_RID as u32
}

fn verify_worker_privileges(token: &OwnedHandle) -> p::Result<()> {
    let mut change_notify = LUID::default();
    if unsafe { LookupPrivilegeValueW(null(), SE_CHANGE_NOTIFY_NAME, &mut change_notify) } == 0 {
        return Err(native_error(
            "safe worker privilege identity cannot be resolved",
        ));
    }
    // The process is still suspended. Remove, then independently re-read;
    // disabled privileges must not be re-enabled by a compromised worker.
    for remove in [true, false] {
        let storage = token_information(token, TokenPrivileges)?;
        let count = read_struct::<u32>(&storage)? as usize;
        let bytes = count
            .checked_mul(size_of::<LUID_AND_ATTRIBUTES>())
            .and_then(|n| n.checked_add(size_of::<u32>()))
            .ok_or_else(|| native_error("worker privilege list is invalid"))?;
        if count > 64 || bytes > size_of_val(storage.as_slice()) {
            return Err(native_error(
                "worker privilege list is unbounded or truncated",
            ));
        }
        for index in 0..count {
            let entry = unsafe {
                ((storage.as_ptr() as *const u8)
                    .add(size_of::<u32>() + index * size_of::<LUID_AND_ATTRIBUTES>())
                    as *const LUID_AND_ATTRIBUTES)
                    .read_unaligned()
            };
            if entry.Luid.LowPart == change_notify.LowPart
                && entry.Luid.HighPart == change_notify.HighPart
            {
                continue;
            }
            if !remove {
                return Err(native_error(
                    "worker token contains an unapproved privilege",
                ));
            }
            let restricted = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: entry.Luid,
                    Attributes: SE_PRIVILEGE_REMOVED,
                }],
            };
            unsafe { SetLastError(0) };
            let changed = unsafe {
                AdjustTokenPrivileges(raw(token), 0, &restricted, 0, null_mut(), null_mut())
            };
            if changed == 0 || unsafe { GetLastError() } != 0 {
                return Err(native_error("worker privilege could not be removed"));
            }
        }
    }
    Ok(())
}

fn process_exited(process: HANDLE) -> p::Result<bool> {
    match unsafe { WaitForSingleObject(process, 0) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        WAIT_FAILED => Err(native_error("worker process wait state is unknown")),
        _ => Err(p::Error("worker process wait state is invalid".into())),
    }
}

fn wait_for_process(process: HANDLE) -> p::Result<()> {
    match unsafe { WaitForSingleObject(process, CANCEL_WAIT.as_millis() as u32) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(p::Error(
            "worker process did not terminate within the bound".into(),
        )),
        WAIT_FAILED => Err(native_error("worker process termination is unknown")),
        _ => Err(p::Error(
            "worker process termination state is invalid".into(),
        )),
    }
}

fn wait_for_job_empty(job: HANDLE) -> p::Result<()> {
    // A Job Object is not signaled whenever its last process exits. Recovery
    // only needs QUERY access and proves emptiness from actual accounting.
    let deadline = Instant::now() + CANCEL_WAIT;
    loop {
        if active_processes(job)? == 0 {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(p::Error(
                "worker Job Object did not become empty within the bound".into(),
            ));
        }
        std::thread::sleep(remaining.min(Duration::from_millis(20)));
    }
}

fn exit_code(process: HANDLE) -> p::Result<u32> {
    let mut code = 0u32;
    if unsafe { GetExitCodeProcess(process, &mut code) } == 0 {
        return Err(native_error("worker process exit code is unavailable"));
    }
    if code == STILL_ACTIVE {
        return Err(p::Error("worker process is still active".into()));
    }
    Ok(code)
}

fn active_processes(job: HANDLE) -> p::Result<u32> {
    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    if unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            &mut info as *mut _ as *mut c_void,
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            null_mut(),
        )
    } == 0
    {
        return Err(native_error(
            "worker Job Object process count is unavailable",
        ));
    }
    Ok(info.ActiveProcesses)
}

fn token_information(token: &OwnedHandle, class: i32) -> p::Result<Vec<usize>> {
    let mut bytes = 0u32;
    unsafe { GetTokenInformation(raw(token), class, null_mut(), 0, &mut bytes) };
    if bytes == 0 && unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
        return Err(native_error("worker token information size is unavailable"));
    }
    let words = (bytes as usize).div_ceil(size_of::<usize>());
    if words == 0 {
        return Err(p::Error("worker token information is empty".into()));
    }
    let mut storage = vec![0usize; words];
    if unsafe {
        GetTokenInformation(
            raw(token),
            class,
            storage.as_mut_ptr() as *mut c_void,
            (storage.len() * size_of::<usize>()) as u32,
            &mut bytes,
        )
    } == 0
    {
        return Err(native_error("worker token information could not be read"));
    }
    Ok(storage)
}

fn read_struct<T: Copy>(storage: &[usize]) -> p::Result<T> {
    if size_of_val(storage) < size_of::<T>() {
        return Err(p::Error("native token information is truncated".into()));
    }
    Ok(unsafe { (storage.as_ptr() as *const T).read_unaligned() })
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
    use super::{
        create_job, is_low_integrity_sid, owned_handle, raw, read_struct, wait_for_job_empty, wide,
    };
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Security::SECURITY_MANDATORY_LABEL_AUTHORITY;
    use windows_sys::Win32::System::JobObjects::OpenJobObjectW;
    use windows_sys::Win32::System::SystemServices::{
        JOB_OBJECT_QUERY, SECURITY_MANDATORY_LOW_RID,
    };

    #[test]
    fn empty_job_recovery_uses_accounting_without_synchronize_access() {
        let identity = tempfile::tempdir().unwrap();
        let name = format!(
            "Local\\ForMeJobRecovery-{}",
            identity.path().file_name().unwrap().to_string_lossy()
        );
        let _owner = create_job(&name).unwrap();
        let query = unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, wide(&name).as_ptr()) };
        let query = owned_handle(query, "query-only test handle").unwrap();
        wait_for_job_empty(raw(&query)).unwrap();
        assert!(wait_for_job_empty(INVALID_HANDLE_VALUE).is_err());
    }

    #[test]
    fn worker_integrity_requires_the_exact_low_mandatory_sid() {
        let authority = SECURITY_MANDATORY_LABEL_AUTHORITY.Value;
        assert!(is_low_integrity_sid(
            authority,
            1,
            SECURITY_MANDATORY_LOW_RID as u32
        ));
        assert!(!is_low_integrity_sid(authority, 1, 0));
        assert!(!is_low_integrity_sid(authority, 1, 8192));
        assert!(!is_low_integrity_sid(
            [0; 6],
            1,
            SECURITY_MANDATORY_LOW_RID as u32
        ));
        assert!(!is_low_integrity_sid(
            authority,
            2,
            SECURITY_MANDATORY_LOW_RID as u32
        ));
    }

    #[test]
    fn native_failure_handles_are_rejected_and_empty_capabilities_need_no_array() {
        assert!(owned_handle(std::ptr::null_mut(), "invalid").is_err());
        assert!(owned_handle(INVALID_HANDLE_VALUE, "invalid").is_err());
        assert!(read_struct::<u32>(&[]).is_err());
        assert_eq!(read_struct::<u32>(&[0]).unwrap(), 0);
    }
}
