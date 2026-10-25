#![cfg(windows)]
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::Path;
use std::ptr::{null, null_mut};

use forme_protocol as p;
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HANDLE, HLOCAL, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Authorization::{
    BuildTrusteeWithSidW, GetEffectiveRightsFromAclW, GetSecurityInfo, SetEntriesInAclW,
    SetSecurityInfo, EXPLICIT_ACCESS_W, GRANT_ACCESS, REVOKE_ACCESS, SE_FILE_OBJECT, TRUSTEE_W,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, PSID};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_NORMAL,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    READ_CONTROL, WRITE_DAC,
};

use super::sandbox_lease::{
    SandboxAclMutation, SandboxAclOperation, SandboxAclReadback, SandboxFileIdentity,
    SandboxTargetAccess,
};

pub(crate) struct Profile {
    name: String,
    sid: OwnedSid,
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
    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    if unsafe { GetFileInformationByHandle(raw(handle), info.as_mut_ptr()) } == 0 {
        return Err(native_error("sandbox target file identity cannot be read"));
    }
    let info = unsafe { info.assume_init() };
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(p::Error("sandbox ACL target is a reparse point".into()));
    }
    let mut file_id = [0u8; 16];
    file_id[..4].copy_from_slice(&info.nFileIndexHigh.to_le_bytes());
    file_id[4..8].copy_from_slice(&info.nFileIndexLow.to_le_bytes());
    SandboxFileIdentity::new(info.dwVolumeSerialNumber as u64, file_id)
}

fn access_mask(access: SandboxTargetAccess) -> u32 {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    };
    match access {
        SandboxTargetAccess::WorkspaceReadWrite => FILE_GENERIC_READ | FILE_GENERIC_WRITE,
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

#[cfg(test)]
mod tests {
    use super::super::sandbox_lease::{
        SandboxLeaseBinding, SandboxLeaseJournal, SandboxProcessTreeTermination,
    };
    use super::*;

    #[test]
    fn failed_file_opens_never_become_owned_handles() {
        assert!(owned_handle(null_mut(), "invalid").is_err());
        assert!(owned_handle(INVALID_HANDLE_VALUE, "invalid").is_err());
        let directory = tempfile::tempdir().unwrap();
        assert!(file_identity(&directory.path().join("missing")).is_err());
    }

    #[test]
    fn real_acl_grant_and_revoke_match_the_lease_readback_contract() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = directory.path().join("runtime");
        let run = runtime.join("job");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::create_dir(&workspace).unwrap();
        let plan = p::canonical_digest(&"acl-native-roundtrip").unwrap();
        let identity = super::super::windows_appcontainer_support::profile_name_for(
            runtime.to_str().unwrap(),
            "job-acl",
            &p::PlanDigest(plan.0.clone()),
        )
        .unwrap();
        // Deriving a SID does not create a profile or change any real workspace.
        let profile = resolve_profile(&identity).unwrap();
        let journal = SandboxLeaseJournal::open(&runtime.join("sandbox-leases.jsonl")).unwrap();
        let lease = journal
            .begin_lease(
                SandboxLeaseBinding::new(
                    "job-acl",
                    plan,
                    identity,
                    &run,
                    file_identity(&run).unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
        let grant = journal
            .begin_acl_grant(
                &lease,
                &workspace,
                file_identity(&workspace).unwrap(),
                SandboxTargetAccess::WorkspaceReadWrite,
            )
            .unwrap();
        let granted = apply_acl(&grant, &profile).unwrap();
        assert!(granted.target_access_effective);
        journal.record_acl_readback(&grant, granted).unwrap();
        // No process is launched in this DACL-only test. Revocation still has
        // to respect the journal's process-tree-before-cleanup ordering.
        journal
            .record_process_tree_terminated(
                &lease,
                SandboxProcessTreeTermination {
                    schema_version: p::SchemaVersion(1),
                    job_id: grant.job_id.clone(),
                    plan_digest: grant.plan_digest.clone(),
                    appcontainer_identity: grant.appcontainer_identity.clone(),
                    root_exit_observed: true,
                    active_processes: 0,
                    observed_at: 1,
                },
            )
            .unwrap();
        let revoke = journal
            .begin_acl_revoke(&lease, &grant.operation_id)
            .unwrap();
        let revoked = apply_acl(&revoke, &profile).unwrap();
        assert!(!revoked.target_access_effective);
        journal.record_acl_readback(&revoke, revoked).unwrap();
        assert!(
            !apply_acl(&revoke, &profile)
                .unwrap()
                .target_access_effective
        );
    }
}
