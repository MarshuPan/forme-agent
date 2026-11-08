#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use forme_protocol as p;
use serde_json::{json, Value};

use super::super::sandbox_lease::{
    SandboxAclMutation, SandboxAclOperation, SandboxRecoveryStage, SandboxTargetAccess,
};
use super::super::windows_appcontainer_acl as acl;

type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;

#[test]
#[ignore = "resumes only recorded cleanup from the private PTY observer failure; never redispatches"]
fn recover_original_pty_probe_observer_cleanup() {
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/closure-inventory-evidence");
    let mut recovered = 0;
    for entry in fs::read_dir(&evidence).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir()
            || !entry
                .file_name()
                .to_string_lossy()
                .starts_with("isolated-pty-")
        {
            continue;
        }
        let root = entry.path();
        let receipt_path = root.join("mechanism-receipt.json");
        if !receipt_path.is_file() || root.join("observer-cleanup-recovery.json").exists() {
            continue;
        }
        let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
        if receipt["cleanup"] != "UNVERIFIABLE"
            || receipt["cleanup_error"] != "probe process image cannot be independently read"
            || receipt["all_observed_tokens_and_job_membership"] != "PASS"
        {
            continue;
        }
        recover(&root, &receipt).unwrap();
        recovered += 1;
    }
    println!("original observer cleanup recoveries: {recovered}");
}

fn recover(root: &Path, receipt: &Value) -> ProbeResult<()> {
    if receipt["schema_version"] != 1
        || receipt["installed_acceptance"] != "NOT_RUN"
        || receipt["pre_resume_token_and_single_job_root"] != "PASS"
        || receipt["terminal_host_isolation"] != "PASS"
    {
        return Err("original experiment scope is not independently verified".into());
    }
    let root = fs::canonicalize(root)?;
    let profile_name = format!("forme-job-{}", root.file_name().unwrap().to_string_lossy());
    if receipt["fixture_digest"]
        != json!(p::sha256_content_digest(&fs::read(
            root.join("program/probe.exe")
        )?))
    {
        return Err("original fixture bytes differ".into());
    }
    let bytes = fs::read(root.join("probe-lease.jsonl"))?;
    if bytes.len() > 64 * 1024 {
        return Err("original experiment lease exceeds its bound".into());
    }
    let rows = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(serde_json::from_slice::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    let grants: Vec<SandboxAclMutation> = rows
        .iter()
        .filter_map(|row| row.get("intent"))
        .map(|value| serde_json::from_value(value.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    let expected = [
        ("workspace", SandboxTargetAccess::WorkspaceReadWrite),
        (
            "program/probe.exe",
            SandboxTargetAccess::RegisteredProgramReadExecute,
        ),
        (
            "inputs/probe-input.json",
            SandboxTargetAccess::JobInputReadOnly,
        ),
        ("inputs/permit.json", SandboxTargetAccess::JobInputReadOnly),
    ];
    if grants.len() != expected.len() {
        return Err(
            "original experiment has unexpected or already partial recovery mutations".into(),
        );
    }
    for (index, (grant, (relative, access))) in grants.iter().zip(expected).enumerate() {
        if grant.schema_version != p::SchemaVersion(1)
            || grant.lease_id != profile_name
            || grant.job_id != profile_name
            || grant.appcontainer_identity != profile_name
            || grant.operation_id != format!("probe-grant-{index}")
            || grant.operation != SandboxAclOperation::Grant
            || grant.recovery_stage != SandboxRecoveryStage::GrantAclPending
            || grant.plan_digest != p::canonical_digest(&("isolated-pty-mechanism", &profile_name))?
            || grant.target_path != fs::canonicalize(root.join(relative))?.to_string_lossy()
            || grant.target_access != access
            || grant.target_file_identity != acl::file_identity(&root.join(relative))?
        {
            return Err("original experiment lease identity or exact target differs".into());
        }
    }
    let job = format!("Local\\ForMePtyProbe-{profile_name}");
    // All original observed processes were independently in this kill-on-close
    // Job. Recovery touches that original named Job only; it starts no program.
    super::recover_named_job(&job)?;
    let profile = acl::resolve_profile(&profile_name)?;
    let mut lease = OpenOptions::new()
        .append(true)
        .open(root.join("probe-lease.jsonl"))?;
    let mut readbacks = Vec::new();
    for grant in grants.iter().rev() {
        let mut revoke = grant.clone();
        revoke.operation = SandboxAclOperation::Revoke;
        revoke.recovery_stage = SandboxRecoveryStage::RevokeAclPending;
        revoke.operation_id = format!("observer-recovery-{}", revoke.operation_id);
        lease.write_all(&serde_json::to_vec(&json!({"intent": revoke}))?)?;
        lease.write_all(b"\n")?;
        lease.sync_all()?;
        let readback = acl::apply_acl(&revoke, &profile)?;
        lease.write_all(&serde_json::to_vec(&json!({"readback": readback}))?)?;
        lease.write_all(b"\n")?;
        lease.sync_all()?;
        readbacks.push(readback);
    }
    let deleted = acl::delete_profile(&profile)?;
    let recovery = json!({
        "schema_version": 1, "operation": "original_pty_probe_observer_cleanup",
        "original_receipt_digest": p::sha256_content_digest(&fs::read(root.join("mechanism-receipt.json"))?),
        "original_named_job_quiescent": true, "acl_readbacks": readbacks,
        "profile_deleted": deleted, "programs_started": 0, "original_result_rewritten": false,
        "installed_acceptance": "NOT_RUN",
    });
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("observer-cleanup-recovery.json"))?;
    file.write_all(&serde_json::to_vec(&recovery)?)?;
    file.sync_all()?;
    Ok(())
}
