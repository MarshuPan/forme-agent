use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::super::reject_linked_ancestors;
use super::projection::{
    calculate_lease_id, calculate_mutation_id, invalid_journal, storage_error, LeaseEnvelope,
    LeaseFact, Projection,
};
use super::{
    SandboxAclMutation, SandboxAclOperation, SandboxAclReadback, SandboxFileIdentity,
    SandboxLeaseBinding, SandboxLeaseRecoveryView, SandboxProcessTreeTermination,
    SandboxRecoveryStage, SandboxTargetAccess, JOURNAL_FILE, MAX_JOURNAL_BYTES, MAX_RECORD_BYTES,
};
use forme_protocol as p;

struct Writer {
    file: File,
    projection: Projection,
    poisoned: bool,
}

/// One OS-locked writer for exact, per-job sandbox ACL lease recovery.
pub struct SandboxLeaseJournal {
    runtime_root: PathBuf,
    writer: Mutex<Writer>,
}

impl SandboxLeaseJournal {
    /// Opens the fixed journal path `<runtime_root>/sandbox-leases.jsonl`.
    pub fn open(path: &Path) -> p::Result<Self> {
        if path.file_name().and_then(|name| name.to_str()) != Some(JOURNAL_FILE) {
            return Err(p::Error(
                "sandbox lease journal path is not canonical".into(),
            ));
        }
        let parent = path
            .parent()
            .ok_or_else(|| p::Error("sandbox lease runtime root is unavailable".into()))?;
        reject_linked_ancestors(parent)?;
        let runtime_root = fs::canonicalize(parent)
            .map_err(|_| p::Error("sandbox lease runtime root cannot be resolved".into()))?;
        if !runtime_root.is_dir() {
            return Err(p::Error(
                "sandbox lease runtime root is not a directory".into(),
            ));
        }
        let path = runtime_root.join(JOURNAL_FILE);
        reject_linked_ancestors(&path)?;

        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|_| storage_error())?;
        file.try_lock()
            .map_err(|_| p::Error("sandbox lease journal already has an active writer".into()))?;
        if file.metadata().map_err(|_| storage_error())?.len() > MAX_JOURNAL_BYTES {
            return Err(p::Error(
                "sandbox lease journal requires offline archival".into(),
            ));
        }

        let mut projection = Projection::default();
        let mut reader = BufReader::new(&file);
        loop {
            let mut bytes = Vec::new();
            let count = reader
                .by_ref()
                .take((MAX_RECORD_BYTES + 1) as u64)
                .read_until(b'\n', &mut bytes)
                .map_err(|_| storage_error())?;
            if count == 0 {
                break;
            }
            if count > MAX_RECORD_BYTES || bytes.last() != Some(&b'\n') {
                return Err(invalid_journal());
            }
            let envelope: LeaseEnvelope =
                serde_json::from_slice(&bytes).map_err(|_| invalid_journal())?;
            projection.fold(&envelope)?;
        }

        Ok(Self {
            runtime_root,
            writer: Mutex::new(Writer {
                file,
                projection,
                poisoned: false,
            }),
        })
    }

    /// Records a new job binding before any ACL or process side effect.
    pub fn begin_lease(&self, mut binding: SandboxLeaseBinding) -> p::Result<String> {
        binding.validate()?;
        let job_directory = canonical_existing_path(Path::new(&binding.job_directory))?;
        let path = PathBuf::from(&job_directory);
        if path.parent() != Some(self.runtime_root.as_path())
            || !fs::metadata(&path)
                .map_err(|_| p::Error("sandbox job directory is unavailable".into()))?
                .is_dir()
        {
            return Err(p::Error(
                "sandbox job directory must be a direct child of runtime root".into(),
            ));
        }
        binding.job_directory = job_directory;
        let lease_id = calculate_lease_id(&binding)?;
        self.append(LeaseFact::LeaseOpened {
            lease_id: lease_id.clone(),
            binding,
        })?;
        Ok(lease_id)
    }

    /// Persists a grant intent. Callers must not mutate an ACL first.
    pub fn begin_acl_grant(
        &self,
        lease_id: &str,
        target_path: &Path,
        target_file_identity: SandboxFileIdentity,
        target_access: SandboxTargetAccess,
    ) -> p::Result<SandboxAclMutation> {
        target_file_identity.validate()?;
        let target_path = canonical_existing_path(target_path)?;
        let record = self.lease(lease_id)?;
        let mut mutation = SandboxAclMutation {
            schema_version: p::SchemaVersion(1),
            lease_id: lease_id.to_owned(),
            operation_id: String::new(),
            job_id: record.binding.job_id.clone(),
            plan_digest: record.binding.plan_digest.clone(),
            appcontainer_identity: record.binding.appcontainer_identity.clone(),
            operation: SandboxAclOperation::Grant,
            recovery_stage: SandboxRecoveryStage::GrantAclPending,
            target_path,
            target_file_identity,
            target_access,
        };
        mutation.operation_id = calculate_mutation_id(&mutation)?;

        let writer = self.writer.lock().map_err(|_| storage_error())?;
        let lease = writer
            .projection
            .leases
            .get(lease_id)
            .ok_or_else(invalid_journal)?;
        if let Some(existing) = lease.mutations.get(&mutation.operation_id) {
            if existing.mutation == mutation {
                return Err(p::Error(
                    "sandbox ACL grant intent already exists; inspect its recovery state".into(),
                ));
            }
            return Err(invalid_journal());
        }
        drop(writer);
        self.append(LeaseFact::AclMutationIntent {
            mutation: mutation.clone(),
        })?;
        Ok(mutation)
    }

    /// Persists a revoke intent from the original grant, preserving its exact path and identity.
    pub fn begin_acl_revoke(
        &self,
        lease_id: &str,
        grant_operation_id: &str,
    ) -> p::Result<SandboxAclMutation> {
        let record = self.lease(lease_id)?;
        let grant = record
            .mutations
            .get(grant_operation_id)
            .ok_or_else(|| p::Error("sandbox ACL grant intent does not exist".into()))?;
        if grant.mutation.operation != SandboxAclOperation::Grant {
            return Err(p::Error("sandbox ACL revoke must reference a grant".into()));
        }
        let mut mutation = grant.mutation.clone();
        mutation.operation = SandboxAclOperation::Revoke;
        mutation.recovery_stage = SandboxRecoveryStage::RevokeAclPending;
        mutation.operation_id = calculate_mutation_id(&mutation)?;
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if record.mutations.contains_key(&mutation.operation_id) {
            return Err(p::Error(
                "sandbox ACL revoke intent already exists; inspect its recovery state".into(),
            ));
        }
        drop(writer);
        self.append(LeaseFact::AclMutationIntent {
            mutation: mutation.clone(),
        })?;
        Ok(mutation)
    }

    /// Persists an OS ACL readback after checking exact identity and expected access.
    pub fn record_acl_readback(
        &self,
        mutation: &SandboxAclMutation,
        readback: SandboxAclReadback,
    ) -> p::Result<()> {
        readback.validate_for(mutation)?;
        self.append(LeaseFact::AclReadback {
            mutation: mutation.clone(),
            readback,
        })
    }

    /// Records that the exact worker job object is empty and its root exited.
    pub fn record_process_tree_terminated(
        &self,
        lease_id: &str,
        proof: SandboxProcessTreeTermination,
    ) -> p::Result<()> {
        let record = self.lease(lease_id)?;
        proof.validate_for(lease_id, &record.binding)?;
        self.append(LeaseFact::ProcessTreeTerminated {
            lease_id: lease_id.to_owned(),
            proof,
        })
    }

    /// Writes the removal intent. The OS identity must be checked absent before confirmation.
    pub fn begin_appcontainer_removal(&self, lease_id: &str) -> p::Result<()> {
        let record = self.lease(lease_id)?;
        self.append(LeaseFact::AppContainerRemovalIntent {
            lease_id: lease_id.to_owned(),
            appcontainer_identity: record.binding.appcontainer_identity,
        })
    }

    pub fn record_appcontainer_removed(
        &self,
        lease_id: &str,
        appcontainer_identity: &str,
        verified_absent: bool,
    ) -> p::Result<()> {
        let record = self.lease(lease_id)?;
        if !verified_absent || record.binding.appcontainer_identity != appcontainer_identity {
            return Err(p::Error(
                "AppContainer identity removal is not verified".into(),
            ));
        }
        self.append(LeaseFact::AppContainerRemoved {
            lease_id: lease_id.to_owned(),
            appcontainer_identity: appcontainer_identity.to_owned(),
        })
    }

    /// Writes the exact job-directory removal intent before deletion.
    pub fn begin_job_directory_removal(&self, lease_id: &str) -> p::Result<()> {
        let record = self.lease(lease_id)?;
        self.append(LeaseFact::JobDirectoryRemovalIntent {
            lease_id: lease_id.to_owned(),
            path: record.binding.job_directory,
            file_identity: record.binding.job_directory_identity,
        })
    }

    /// Confirms the bound directory is absent; a replacement or changed identity fails closed.
    pub fn record_job_directory_removed(&self, lease_id: &str) -> p::Result<()> {
        let record = self.lease(lease_id)?;
        let path = PathBuf::from(&record.binding.job_directory);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(p::Error(
                    "sandbox job directory still exists after cleanup".into(),
                ));
            }
            Err(_) => return Err(storage_error()),
        }
        self.append(LeaseFact::JobDirectoryRemoved {
            lease_id: lease_id.to_owned(),
            path: record.binding.job_directory,
            file_identity: record.binding.job_directory_identity,
        })
    }

    pub fn record_cleanup_receipt(&self, lease_id: &str) -> p::Result<()> {
        self.append(LeaseFact::CleanupReceipt {
            lease_id: lease_id.to_owned(),
        })
    }

    /// Returns every lease whose cleanup receipt has not been durably recorded.
    pub fn pending_cleanup(&self) -> p::Result<Vec<SandboxLeaseRecoveryView>> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        Ok(writer
            .projection
            .leases
            .iter()
            .filter(|(_, record)| !record.cleanup_complete)
            .map(|(lease_id, record)| record.view(lease_id))
            .collect())
    }

    /// Startup admission gate: every lease from the prior process must be cleaned first.
    pub fn verify_startup_recovery_complete(&self) -> p::Result<()> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer
            .projection
            .leases
            .values()
            .any(|record| !record.cleanup_complete)
        {
            return Err(p::Error(
                "sandbox lease recovery is required; worker admission is blocked".into(),
            ));
        }
        Ok(())
    }

    fn lease(&self, lease_id: &str) -> p::Result<super::projection::LeaseRecord> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        writer
            .projection
            .leases
            .get(lease_id)
            .cloned()
            .ok_or_else(|| p::Error("sandbox lease does not exist".into()))
    }

    fn append(&self, fact: LeaseFact) -> p::Result<()> {
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        let fact_digest = p::canonical_digest(&fact)?;
        if writer.projection.fact_digests.contains(&fact_digest) {
            if matches!(
                fact,
                LeaseFact::LeaseOpened { .. } | LeaseFact::AclMutationIntent { .. }
            ) {
                return Err(p::Error(
                    "sandbox intent already exists; inspect its recovery state".into(),
                ));
            }
            return Ok(());
        }
        let lease_seq = writer
            .projection
            .sequence
            .checked_add(1)
            .ok_or_else(invalid_journal)?;
        let previous = writer.projection.head()?;
        let envelope = LeaseEnvelope {
            schema_version: p::SchemaVersion(1),
            lease_seq,
            digest: LeaseEnvelope::calculate(lease_seq, &previous, &fact)?,
            previous,
            fact,
        };
        let mut next = writer.projection.clone();
        next.fold(&envelope)?;
        let mut bytes = serde_json::to_vec(&envelope).map_err(|_| invalid_journal())?;
        bytes.push(b'\n');
        if bytes.len() > MAX_RECORD_BYTES
            || writer.file.metadata().map_err(|_| storage_error())?.len() + bytes.len() as u64
                > MAX_JOURNAL_BYTES
        {
            return Err(storage_error());
        }
        writer.poisoned = true;
        writer
            .file
            .seek(SeekFrom::End(0))
            .map_err(|_| storage_error())?;
        writer.file.write_all(&bytes).map_err(|_| storage_error())?;
        writer.file.sync_all().map_err(|_| storage_error())?;
        writer.projection = next;
        writer.poisoned = false;
        Ok(())
    }
}

impl SandboxAclReadback {
    pub(super) fn validate_for(&self, mutation: &SandboxAclMutation) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
            || self.operation_id != mutation.operation_id
            || self.target_file_identity != mutation.target_file_identity
        {
            return Err(p::Error("sandbox ACL readback identity mismatch".into()));
        }
        p::validate_product_digest(&self.effective_acl_digest)?;
        let expected = mutation.operation == SandboxAclOperation::Grant;
        if self.target_access_effective != expected {
            return Err(p::Error(
                "sandbox ACL readback does not match the requested mutation".into(),
            ));
        }
        Ok(())
    }
}

impl SandboxProcessTreeTermination {
    pub(super) fn validate_for(
        &self,
        lease_id: &str,
        binding: &SandboxLeaseBinding,
    ) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
            || self.job_id != binding.job_id
            || self.plan_digest != binding.plan_digest
            || self.appcontainer_identity != binding.appcontainer_identity
            || !self.root_exit_observed
            || self.active_processes != 0
            || self.observed_at <= 0
        {
            return Err(p::Error(format!(
                "sandbox process tree termination is not verified for {lease_id}"
            )));
        }
        Ok(())
    }
}

fn canonical_existing_path(path: &Path) -> p::Result<String> {
    reject_linked_ancestors(path)?;
    let canonical = fs::canonicalize(path)
        .map_err(|_| p::Error("sandbox target path cannot be resolved".into()))?;
    canonical
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| p::Error("sandbox target path is not valid Unicode".into()))
}
