#![cfg(windows)]
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::File;
use std::mem::{size_of, MaybeUninit};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::Path;
use std::ptr::{null, null_mut};

use forme_protocol as p;
use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HANDLE, HLOCAL, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    BuildTrusteeWithSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    GetEffectiveRightsFromAclW, GetSecurityInfo, SetEntriesInAclW, SetSecurityInfo,
    EXPLICIT_ACCESS_W, GRANT_ACCESS, REVOKE_ACCESS, SE_FILE_OBJECT, TRUSTEE_W,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows_sys::Win32::Security::{
    EqualSid, GetAce, GetSecurityDescriptorSacl, GetTokenInformation, IsWellKnownSid, TokenUser,
    WinLocalSystemSid, ACL, DACL_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION, PSID,
    SYSTEM_MANDATORY_LABEL_ACE, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FileDispositionInfo, GetFileInformationByHandle, SetFileInformationByHandle,
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    READ_CONTROL, WRITE_DAC, WRITE_OWNER,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::sandbox_lease::{
    SandboxAclMutation, SandboxAclOperation, SandboxAclReadback, SandboxFileIdentity,
    SandboxTargetAccess,
};

pub(crate) struct Profile {
    name: String,
    sid: OwnedSid,
}

/// Managed-service identity proof rejects command-line claims after SCM binding.
pub(crate) fn verify_current_process_is_local_system() -> p::Result<()> {
    let mut token = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(native_error(
            "managed worker host token could not be opened",
        ));
    }
    let token = owned_handle(token, "managed worker host token handle is invalid")?;
    let mut bytes = 0u32;
    unsafe {
        GetTokenInformation(raw(&token), TokenUser, null_mut(), 0, &mut bytes);
    }
    if bytes == 0 && unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
        return Err(native_error(
            "managed worker host token information size is unavailable",
        ));
    }
    let mut storage = vec![0u8; bytes as usize];
    if unsafe {
        GetTokenInformation(
            raw(&token),
            TokenUser,
            storage.as_mut_ptr() as *mut c_void,
            bytes,
            &mut bytes,
        )
    } == 0
    {
        return Err(native_error(
            "managed worker host token information could not be read",
        ));
    }
    if storage.len() < size_of::<TOKEN_USER>() {
        return Err(native_error(
            "managed worker host token information is truncated",
        ));
    }
    let user = unsafe { (storage.as_ptr() as *const TOKEN_USER).read_unaligned() };
    if user.User.Sid.is_null() || unsafe { IsWellKnownSid(user.User.Sid, WinLocalSystemSid) } == 0 {
        return Err(p::Error(
            "managed worker host must run as LocalSystem".into(),
        ));
    }
    Ok(())
}

impl Profile {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn raw_sid(&self) -> PSID {
        self.sid.raw
    }
}

struct OwnedSid {
    raw: PSID,
}

// The SID allocation is owned exclusively by this value and is not tied to a
// thread or apartment. Moving the owner between the worker registry and the
// cleanup path is therefore safe; concurrent access is still prevented by the
// enclosing worker mutex.
unsafe impl Send for OwnedSid {}

impl Drop for OwnedSid {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                windows_sys::Win32::Security::FreeSid(self.raw);
            }
        }
    }
}

pub(crate) fn create_profile(name: &str) -> p::Result<Profile> {
    let wide_name = wide(name);
    let mut sid = null_mut();
    let result = unsafe {
        CreateAppContainerProfile(
            wide_name.as_ptr(),
            wide_name.as_ptr(),
            wide_name.as_ptr(),
            null(),
            0,
            &mut sid,
        )
    };
    if result < 0 || sid.is_null() {
        return Err(native_error(
            "AppContainer profile could not be created; existing identity requires recovery",
        ));
    }
    Ok(Profile {
        name: name.to_owned(),
        sid: OwnedSid { raw: sid },
    })
}

pub(crate) fn resolve_profile(name: &str) -> p::Result<Profile> {
    let mut sid = null_mut();
    let status =
        unsafe { DeriveAppContainerSidFromAppContainerName(wide(name).as_ptr(), &mut sid) };
    if status < 0 || sid.is_null() {
        return Err(native_error(
            "AppContainer identity cannot be re-derived from its journal binding",
        ));
    }
    Ok(Profile {
        name: name.to_owned(),
        sid: OwnedSid { raw: sid },
    })
}

pub(crate) fn delete_profile(profile: &Profile) -> p::Result<bool> {
    let result = unsafe { DeleteAppContainerProfile(wide(profile.name()).as_ptr()) };
    if result == 0 {
        return Ok(true);
    }
    // Only the documented Win32 HRESULTs prove absence. A matching low word
    // from a different facility must not turn an unknown deletion into success.
    if matches!(result as u32, 0x8007_0002 | 0x8007_0490) {
        return Ok(false);
    }
    Err(native_error(
        "AppContainer profile removal state is unknown; retry recovery",
    ))
}

pub(crate) fn apply_acl(
    mutation: &SandboxAclMutation,
    profile: &Profile,
) -> p::Result<SandboxAclReadback> {
    let target = Path::new(&mutation.target_path);
    let handle = open_acl_target(target)?;
    if file_identity_for_handle(&handle)? != mutation.target_file_identity {
        return Err(p::Error("sandbox ACL target identity changed".into()));
    }
    let mut old_dacl = null_mut();
    let mut descriptor = null_mut();
    let status = unsafe {
        GetSecurityInfo(
            raw(&handle),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut old_dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS || old_dacl.is_null() {
        free_local(descriptor);
        return Err(p::Error(
            "sandbox target DACL is unavailable or unrestricted".into(),
        ));
    }

    let mut trustee = TRUSTEE_W::default();
    unsafe { BuildTrusteeWithSidW(&mut trustee, profile.raw_sid()) };
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: access_mask(mutation.target_access),
        grfAccessMode: if mutation.operation == SandboxAclOperation::Grant {
            GRANT_ACCESS
        } else {
            REVOKE_ACCESS
        },
        grfInheritance: inheritance(mutation.target_access),
        Trustee: trustee,
    };
    let mut new_acl = null_mut();
    let acl_status = unsafe { SetEntriesInAclW(1, &entry, old_dacl, &mut new_acl) };
    if acl_status != ERROR_SUCCESS || new_acl.is_null() {
        free_local(descriptor);
        return Err(p::Error("sandbox target ACL mutation failed".into()));
    }
    let set_status = unsafe {
        SetSecurityInfo(
            raw(&handle),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            new_acl,
            null_mut(),
        )
    };
    free_local(new_acl as *mut c_void);
    free_local(descriptor);
    if set_status != ERROR_SUCCESS {
        return Err(p::Error("sandbox target ACL could not be persisted".into()));
    }

    // DACL readback does not prove Mandatory Integrity Control allows a Low
    // worker to write. It never changes the target's integrity label.
    let rights = effective_rights(&handle, profile.raw_sid())?;
    let expected = mutation.operation == SandboxAclOperation::Grant;
    let verified = if expected {
        rights & access_mask(mutation.target_access) == access_mask(mutation.target_access)
    } else {
        rights & access_mask(mutation.target_access) == 0
    };
    if !verified {
        return Err(p::Error(
            "sandbox ACL readback does not prove the requested access boundary".into(),
        ));
    }
    let digest = p::canonical_digest(&(
        "forme-sandbox-acl-readback-v1",
        &mutation.target_path,
        mutation.operation,
        rights,
        expected,
    ))?;
    Ok(SandboxAclReadback {
        schema_version: p::SchemaVersion(1),
        operation_id: mutation.operation_id.clone(),
        target_file_identity: mutation.target_file_identity.clone(),
        effective_acl_digest: digest,
        target_access_effective: expected,
    })
}

fn effective_rights(handle: &OwnedHandle, sid: PSID) -> p::Result<u32> {
    let mut dacl = null_mut();
    let mut descriptor = null_mut();
    let status = unsafe {
        GetSecurityInfo(
            raw(handle),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS || dacl.is_null() {
        free_local(descriptor);
        return Err(p::Error("sandbox ACL readback DACL is unavailable".into()));
    }
    let mut trustee = TRUSTEE_W::default();
    unsafe { BuildTrusteeWithSidW(&mut trustee, sid) };
    let mut rights = 0u32;
    let result = unsafe { GetEffectiveRightsFromAclW(dacl, &trustee, &mut rights) };
    free_local(descriptor);
    if result != ERROR_SUCCESS {
        return Err(p::Error(
            "sandbox ACL effective-rights readback failed".into(),
        ));
    }
    Ok(rights)
}

pub(crate) fn file_identity(path: &Path) -> p::Result<SandboxFileIdentity> {
    let path = path
        .to_str()
        .ok_or_else(|| p::Error("sandbox path is not valid Unicode".into()))?;
    let handle = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    let handle = owned_handle(handle, "sandbox target identity cannot be opened")?;
    file_identity_for_handle(&handle)
}

fn open_acl_target(path: &Path) -> p::Result<OwnedHandle> {
    let path = path
        .to_str()
        .ok_or_else(|| p::Error("sandbox ACL path is not valid Unicode".into()))?;
    let handle = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            FILE_READ_ATTRIBUTES | READ_CONTROL | WRITE_DAC,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    owned_handle(
        handle,
        "sandbox ACL target cannot be opened for a stable update",
    )
}

fn file_identity_for_handle(handle: &OwnedHandle) -> p::Result<SandboxFileIdentity> {
    inspect_raw(raw(handle)).map(|(identity, _)| identity)
}

pub(crate) fn inspect_file(file: &File) -> p::Result<(SandboxFileIdentity, u32)> {
    inspect_raw(file.as_raw_handle() as HANDLE)
}

fn inspect_raw(handle: HANDLE) -> p::Result<(SandboxFileIdentity, u32)> {
    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    if unsafe { GetFileInformationByHandle(handle, info.as_mut_ptr()) } == 0 {
        return Err(native_error("sandbox target file identity cannot be read"));
    }
    let info = unsafe { info.assume_init() };
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(p::Error("sandbox ACL target is a reparse point".into()));
    }
    let mut file_id = [0u8; 16];
    file_id[..4].copy_from_slice(&info.nFileIndexHigh.to_le_bytes());
    file_id[4..8].copy_from_slice(&info.nFileIndexLow.to_le_bytes());
    Ok((
        SandboxFileIdentity::new(info.dwVolumeSerialNumber as u64, file_id)?,
        info.nNumberOfLinks,
    ))
}

/// The caller owns the original delete-capable handle and has already made
/// the exact writeback intent durable. No mutable path is reopened here.
pub(crate) fn mark_deleted(file: &File) -> p::Result<()> {
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle() as HANDLE,
            FileDispositionInfo,
            &disposition as *const _ as *const c_void,
            std::mem::size_of_val(&disposition) as u32,
        )
    } == 0
    {
        return Err(native_error(
            "workspace handle deletion could not be verified",
        ));
    }
    Ok(())
}

/// Only the brand-new private copy may be passed here. WRITE_OWNER is enough
/// for a Low mandatory label; the original workspace is never relabelled.
pub(crate) fn set_low_integrity(path: &Path) -> p::Result<()> {
    let path = path
        .to_str()
        .ok_or_else(|| native_error("private workspace path is invalid"))?;
    let handle = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            WRITE_OWNER | READ_CONTROL | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    let handle = owned_handle(handle, "private workspace label target cannot be opened")?;
    file_identity_for_handle(&handle)?;
    let mut descriptor = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide("S:(ML;OICI;NW;;;LW)").as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
    {
        return Err(native_error(
            "private workspace Low label cannot be constructed",
        ));
    }
    let mut present = 0;
    let mut defaulted = 0;
    let mut expected = null_mut();
    let got = unsafe {
        GetSecurityDescriptorSacl(descriptor, &mut present, &mut expected, &mut defaulted)
    };
    let result = if got != 0 && present != 0 && !expected.is_null() {
        let status = unsafe {
            SetSecurityInfo(
                raw(&handle),
                SE_FILE_OBJECT,
                LABEL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                null(),
                expected,
            )
        };
        if status == ERROR_SUCCESS {
            verify_label(&handle, expected)
        } else {
            Err(native_error("private workspace Low label could not be set"))
        }
    } else {
        Err(native_error("private workspace Low label is unavailable"))
    };
    free_local(descriptor);
    result
}

fn verify_label(handle: &OwnedHandle, expected: *mut ACL) -> p::Result<()> {
    let mut descriptor = null_mut();
    let mut observed = null_mut();
    let status = unsafe {
        GetSecurityInfo(
            raw(handle),
            SE_FILE_OBJECT,
            LABEL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut observed,
            &mut descriptor,
        )
    };
    let result = if status == ERROR_SUCCESS && !observed.is_null() {
        let mut actual_ace = null_mut();
        let mut expected_ace = null_mut();
        // Both ACLs are OS-returned valid security descriptors, kept alive
        // throughout GetAce/EqualSid. LABEL_SECURITY_INFORMATION filters the
        // SACL to mandatory labels, so an additional ACE is a mismatch.
        let valid = unsafe {
            (*observed).AceCount == 1
                && GetAce(observed, 0, &mut actual_ace) != 0
                && GetAce(expected, 0, &mut expected_ace) != 0
        };
        if valid {
            let actual = actual_ace as *const SYSTEM_MANDATORY_LABEL_ACE;
            let expected = expected_ace as *const SYSTEM_MANDATORY_LABEL_ACE;
            let matches = unsafe {
                (*actual).Header.AceType == (*expected).Header.AceType
                    && (*actual).Mask == (*expected).Mask
                    && EqualSid(
                        std::ptr::addr_of!((*actual).SidStart) as PSID,
                        std::ptr::addr_of!((*expected).SidStart) as PSID,
                    ) != 0
            };
            if matches {
                Ok(())
            } else {
                Err(native_error("private workspace Low label readback differs"))
            }
        } else {
            Err(native_error(
                "private workspace Low label readback is invalid",
            ))
        }
    } else {
        Err(native_error("private workspace Low label cannot be read"))
    };
    free_local(descriptor);
    result
}

fn access_mask(access: SandboxTargetAccess) -> u32 {
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_DELETE_CHILD, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    };
    match access {
        SandboxTargetAccess::WorkspaceReadWrite => {
            FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | FILE_DELETE_CHILD
        }
        SandboxTargetAccess::JobInputReadOnly => FILE_GENERIC_READ,
        SandboxTargetAccess::RegisteredProgramReadExecute
        | SandboxTargetAccess::FixedRuntimeReadExecute => FILE_GENERIC_READ | FILE_GENERIC_EXECUTE,
    }
}

fn inheritance(access: SandboxTargetAccess) -> u32 {
    use windows_sys::Win32::Security::{NO_INHERITANCE, SUB_CONTAINERS_AND_OBJECTS_INHERIT};
    if access == SandboxTargetAccess::WorkspaceReadWrite {
        SUB_CONTAINERS_AND_OBJECTS_INHERIT
    } else {
        NO_INHERITANCE
    }
}

fn owned_handle(handle: HANDLE, message: &'static str) -> p::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(native_error(message));
    }
    // SAFETY: callers pass ownership of a successful Win32 handle creation;
    // neither Win32 failure sentinel can reach OwnedHandle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle() as HANDLE
}

fn free_local(value: *mut c_void) {
    if !value.is_null() {
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(value as HLOCAL);
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn native_error(message: &str) -> p::Error {
    p::Error(message.into())
}

pub(crate) fn local_app_data() -> p::Result<std::path::PathBuf> {
    use windows_sys::Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath};
    let mut path = null_mut();
    if unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, null_mut(), &mut path) } < 0
        || path.is_null()
    {
        return Err(native_error("Windows profile directory cannot be resolved"));
    }
    // The API owns a NUL-terminated, CoTaskMem-allocated UTF-16 string.
    // Bound the scan and free it on every decoding result.
    let result = (|| {
        let mut length = 0;
        while length < 32_768 && unsafe { *path.add(length) } != 0 {
            length += 1;
        }
        if length == 32_768 {
            return Err(native_error("Windows profile path is unbounded"));
        }
        let text = String::from_utf16(unsafe { std::slice::from_raw_parts(path, length) })
            .map_err(|_| native_error("Windows profile path is invalid"))?;
        let path = std::path::PathBuf::from(text);
        if !path.is_absolute() {
            return Err(native_error("Windows profile path is not absolute"));
        }
        Ok(path)
    })();
    unsafe { windows_sys::Win32::System::Com::CoTaskMemFree(path as *const c_void) };
    result
}

#[cfg(test)]
#[path = "windows_appcontainer_acl_tests.rs"]
mod tests;
