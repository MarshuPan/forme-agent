#![cfg(windows)]

use std::collections::BTreeMap;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use forme_protocol as p;

use super::sandbox_lease::SandboxAclMutation;

pub(super) struct WorkerEnvironment<'a> {
    pub job_path: &'a Path,
    pub permit_path: &'a Path,
    pub workspace: &'a Path,
    pub execution_path: Option<&'a Path>,
    pub workspace_scope: &'a str,
    pub deadline: p::Timestamp,
}

pub(super) fn environment_block(
    input: WorkerEnvironment<'_>,
    system_root: &Path,
    local_app_data: &Path,
) -> p::Result<Vec<u16>> {
    let WorkerEnvironment {
        job_path,
        permit_path,
        workspace,
        execution_path,
        workspace_scope,
        deadline,
    } = input;
    let mut values = BTreeMap::from([
        (
            "LOCALAPPDATA",
            path_string(local_app_data, "Windows profile directory")?,
        ),
        (
            "SystemRoot",
            path_string(system_root, "Windows system directory")?,
        ),
        ("FORME_WORKER_PROTOCOL_VERSION", "1".to_owned()),
        (
            "FORME_WORKER_JOB_PATH",
            path_string(job_path, "worker job path")?,
        ),
        (
            "FORME_WORKER_PERMIT_PATH",
            path_string(permit_path, "worker permit path")?,
        ),
        (
            "FORME_WORKER_WORKSPACE_PATH",
            path_string(workspace, "worker workspace path")?,
        ),
    ]);
    if let Some(path) = execution_path {
        values.insert(
            "FORME_WORKER_EXECUTION_PATH",
            path_string(path, "worker execution path")?,
        );
    }
    values.insert("FORME_WORKER_WORKSPACE_SCOPE", workspace_scope.to_owned());
    values.insert("FORME_WORKER_DEADLINE", deadline.to_string());
    let mut block = Vec::new();
    for (key, value) in values {
        block.extend(std::ffi::OsStr::new(&format!("{key}={value}")).encode_wide());
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

pub(super) fn job_object_name(appcontainer_identity: &str) -> p::Result<String> {
    if !appcontainer_identity.starts_with("forme-job-")
        || appcontainer_identity.len() > 64
        || !appcontainer_identity
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(p::Error("AppContainer identity is invalid".into()));
    }
    Ok(format!("Local\\ForMeWorker-{appcontainer_identity}"))
}

pub(super) fn path_string(path: &Path, name: &str) -> p::Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| p::Error(format!("{name} is not valid Unicode")))
}

pub(super) fn profile_name_for(
    runtime_root: &str,
    job_id: &str,
    plan_digest: &p::PlanDigest,
) -> p::Result<String> {
    let digest = p::canonical_digest(&(
        "forme-appcontainer-profile-v1",
        runtime_root,
        job_id,
        plan_digest,
    ))?
    .0;
    let suffix = digest
        .strip_prefix("sha256:")
        .and_then(|value| value.get(..48))
        .ok_or_else(|| p::Error("AppContainer identity digest is invalid".into()))?;
    Ok(format!("forme-job-{suffix}"))
}

pub(super) fn quote_windows_arg(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0usize;
    for character in value.chars() {
        if character == '\\' {
            slashes += 1;
        } else if character == '"' {
            result.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
            result.push('"');
            slashes = 0;
        } else {
            result.extend(std::iter::repeat_n('\\', slashes));
            result.push(character);
            slashes = 0;
        }
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    result
}

pub(super) fn wide(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub(super) fn same_target(left: &SandboxAclMutation, right: &SandboxAclMutation) -> bool {
    left.target_path == right.target_path
        && left.target_file_identity == right.target_file_identity
        && left.target_access == right.target_access
}

pub(super) fn recovery_timestamp() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
        .max(1)
}
