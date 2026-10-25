#![cfg(windows)]

use std::fs;
use std::path::Path;

use forme_protocol as p;

use super::super::reject_linked_ancestors;
use super::super::sandbox_lease::{
    SandboxAclOperation, SandboxFileIdentity, SandboxLeaseJournal, SandboxLeaseRecoveryView,
    SandboxProcessTreeTermination,
};
use super::super::windows_appcontainer_acl as acl;
use super::super::windows_appcontainer_native as native;
use super::super::windows_appcontainer_support::{
    job_object_name, recovery_timestamp, same_target,
};

pub(super) fn recover_pending(journal: &SandboxLeaseJournal) -> p::Result<()> {
    for lease in journal.pending_cleanup()? {
        let profile = acl::resolve_profile(&lease.binding.appcontainer_identity)?;
        if lease.process_tree_termination.is_none() {
            native::recover_named_job(&job_object_name(&lease.binding.appcontainer_identity)?)?;
            journal.record_process_tree_terminated(
                &lease.lease_id,
                SandboxProcessTreeTermination {
                    schema_version: p::SchemaVersion(1),
                    job_id: lease.binding.job_id.clone(),
                    plan_digest: lease.binding.plan_digest.clone(),
                    appcontainer_identity: lease.binding.appcontainer_identity.clone(),
                    root_exit_observed: true,
                    active_processes: 0,
                    observed_at: recovery_timestamp(),
                },
            )?;
        }
        cleanup_lease(
            journal,
            &lease.lease_id,
            Path::new(&lease.binding.job_directory),
            &profile,
            (true, 0),
            recovery_timestamp(),
        )?;
    }
    journal.verify_startup_recovery_complete()
}

pub(super) fn cleanup_lease(
    journal: &SandboxLeaseJournal,
    lease_id: &str,
    run_dir: &Path,
    profile: &acl::Profile,
    terminal: (bool, u32),
    now: p::Timestamp,
) -> p::Result<()> {
    let view = find_pending(journal, lease_id)?;
    if view.process_tree_termination.is_none() {
        let (root_exit_observed, active_processes) = terminal;
        if !root_exit_observed || active_processes != 0 {
            return Err(p::Error(
                "worker process tree is not terminal; sandbox cleanup is blocked".into(),
            ));
        }
        journal.record_process_tree_terminated(
            lease_id,
            SandboxProcessTreeTermination {
                schema_version: p::SchemaVersion(1),
                job_id: view.binding.job_id.clone(),
                plan_digest: view.binding.plan_digest.clone(),
                appcontainer_identity: view.binding.appcontainer_identity.clone(),
                root_exit_observed,
                active_processes,
                observed_at: now.max(1),
            },
        )?;
    }

    let grants = find_pending(journal, lease_id)?
        .acl_mutations
        .into_iter()
        .filter(|state| state.mutation.operation == SandboxAclOperation::Grant)
        .collect::<Vec<_>>();
    for grant in grants {
        let latest = find_pending(journal, lease_id)?;
        let existing_revoke = latest.acl_mutations.iter().find(|candidate| {
            candidate.mutation.operation == SandboxAclOperation::Revoke
                && same_target(&candidate.mutation, &grant.mutation)
        });
        let revoke = match existing_revoke {
            Some(state) if state.readback.is_some() => continue,
            Some(state) => state.mutation.clone(),
            None => journal.begin_acl_revoke(lease_id, &grant.mutation.operation_id)?,
        };
        let readback = acl::apply_acl(&revoke, profile)?;
        journal.record_acl_readback(&revoke, readback)?;
    }

    let latest = find_pending(journal, lease_id)?;
    if !latest.appcontainer_removed {
        if !latest.appcontainer_removal_intent {
            journal.begin_appcontainer_removal(lease_id)?;
        }
        let _profile_removed = acl::delete_profile(profile)?;
        journal.record_appcontainer_removed(lease_id, profile.name(), true)?;
    }

    let latest = find_pending(journal, lease_id)?;
    if !latest.job_directory_removed {
        // Unknown writeback must retain its copy, baseline and original
        // intent. ACL/profile cleanup above is still mandatory on restart.
        if let Some(receipt) =
            super::super::windows_workspace_overlay::verify_cleanup_allowed(run_dir)?
        {
            journal.record_writeback_receipt(lease_id, receipt)?;
        }
        if !latest.job_directory_removal_intent {
            journal.begin_job_directory_removal(lease_id)?;
        }
        remove_bound_job_directory(run_dir, &latest.binding.job_directory_identity)?;
        journal.record_job_directory_removed(lease_id)?;
    }
    journal.record_cleanup_receipt(lease_id)
}

fn remove_bound_job_directory(
    path: &Path,
    expected_identity: &SandboxFileIdentity,
) -> p::Result<()> {
    reject_linked_ancestors(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_dir() || acl::file_identity(path)? != *expected_identity {
                return Err(p::Error(
                    "sandbox job directory identity changed during cleanup".into(),
                ));
            }
            fs::remove_dir_all(path)
                .map_err(|_| p::Error("sandbox job directory cleanup failed".into()))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(p::Error("sandbox job directory cannot be inspected".into())),
    }
    Ok(())
}

fn find_pending(
    journal: &SandboxLeaseJournal,
    lease_id: &str,
) -> p::Result<SandboxLeaseRecoveryView> {
    journal
        .pending_cleanup()?
        .into_iter()
        .find(|view| view.lease_id == lease_id)
        .ok_or_else(|| p::Error("sandbox lease disappeared during recovery".into()))
}
