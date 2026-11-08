#![forbid(unsafe_code)]

use super::{
    create_job, is_low_integrity_sid, owned_handle, query_only_job, raw, read_struct,
    wait_for_job_empty,
};
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Security::SECURITY_MANDATORY_LABEL_AUTHORITY;
use windows_sys::Win32::System::SystemServices::SECURITY_MANDATORY_LOW_RID;

#[test]
fn empty_job_recovery_uses_accounting_without_synchronize_access() {
    let identity = tempfile::tempdir().unwrap();
    let name = format!(
        "Local\\ForMeJobRecovery-{}",
        identity.path().file_name().unwrap().to_string_lossy()
    );
    let _owner = create_job(&name).unwrap();
    let query = query_only_job(&name).unwrap();
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

#[test]
fn process_observer_rejects_a_pid_outside_the_original_job_before_token_access() {
    let name = format!("Local\\ForMeProbeReject-{}", std::process::id());
    let job = create_job(&name).unwrap();
    assert!(super::observe_process(std::process::id(), raw(&job), std::ptr::null_mut()).is_err());
}

#[test]
fn exited_process_observation_uses_its_pinned_handle_and_exit_time() {
    use std::os::windows::io::AsHandle;
    use std::process::{Command, Stdio};
    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("--list")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let handle = child.as_handle().try_clone_to_owned().unwrap();
    assert!(child.wait().unwrap().success());
    let (created, exited, image) = super::process_identity(&handle).unwrap();
    assert!(created > 0);
    assert!(exited);
    assert!(image.is_empty());
}
