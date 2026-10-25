//! Durable host-side recovery facts for per-job Windows sandbox ACL leases.
//!
//! This journal records intent and verified observations only. It never
//! changes ACLs, launches workers, or writes brain events.

use std::path::Path;

use forme_protocol as p;
use serde::{Deserialize, Serialize};

mod journal;
mod projection;

pub use journal::SandboxLeaseJournal;

pub(super) const JOURNAL_FILE: &str = "sandbox-leases.jsonl";
const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxFileIdentity {
    pub volume_serial_number: u64,
    pub file_id: [u8; 16],
}

impl SandboxFileIdentity {
    pub fn new(volume_serial_number: u64, file_id: [u8; 16]) -> p::Result<Self> {
        let identity = Self {
            volume_serial_number,
            file_id,
        };
        identity.validate()?;
        Ok(identity)
    }

    fn validate(&self) -> p::Result<()> {
        if self.volume_serial_number == 0 || self.file_id.iter().all(|byte| *byte == 0) {
            return Err(p::Error("sandbox file identity is invalid".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxLeaseBinding {
    pub schema_version: p::SchemaVersion,
    pub job_id: String,
    pub plan_digest: p::SchemaDigest,
    pub appcontainer_identity: String,
    pub job_directory: String,
    pub job_directory_identity: SandboxFileIdentity,
}

impl SandboxLeaseBinding {
    pub fn new(
        job_id: impl Into<String>,
        plan_digest: p::SchemaDigest,
        appcontainer_identity: impl Into<String>,
        job_directory: &Path,
        job_directory_identity: SandboxFileIdentity,
    ) -> p::Result<Self> {
        let binding = Self {
            schema_version: p::SchemaVersion(1),
            job_id: job_id.into(),
            plan_digest,
            appcontainer_identity: appcontainer_identity.into(),
            job_directory: canonical_existing_path(job_directory)?,
            job_directory_identity,
        };
        binding.validate()?;
        Ok(binding)
    }

    fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1) {
            return Err(p::Error("unsupported sandbox lease schema".into()));
        }
        p::validate_product_id(&self.job_id)
            .map_err(|_| p::Error("sandbox lease job id is invalid".into()))?;
        p::validate_product_digest(&self.plan_digest)?;
        projection::validate_appcontainer_identity(&self.appcontainer_identity)?;
        projection::validate_absolute_normalized_path(&self.job_directory)?;
        self.job_directory_identity.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxAclOperation {
    Grant,
    Revoke,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxTargetAccess {
    WorkspaceReadWrite,
    JobInputReadOnly,
    RegisteredProgramReadExecute,
    FixedRuntimeReadExecute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxRecoveryStage {
    GrantAclPending,
    RevokeAclPending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxAclMutation {
    pub schema_version: p::SchemaVersion,
    pub lease_id: String,
    pub operation_id: String,
    pub job_id: String,
    pub plan_digest: p::SchemaDigest,
    pub appcontainer_identity: String,
    pub operation: SandboxAclOperation,
    pub recovery_stage: SandboxRecoveryStage,
    pub target_path: String,
    pub target_file_identity: SandboxFileIdentity,
    pub target_access: SandboxTargetAccess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxAclReadback {
    pub schema_version: p::SchemaVersion,
    pub operation_id: String,
    pub target_file_identity: SandboxFileIdentity,
    pub effective_acl_digest: p::SchemaDigest,
    pub target_access_effective: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxProcessTreeTermination {
    pub schema_version: p::SchemaVersion,
    pub job_id: String,
    pub plan_digest: p::SchemaDigest,
    pub appcontainer_identity: String,
    pub root_exit_observed: bool,
    pub active_processes: u32,
    pub observed_at: p::Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxAclMutationState {
    pub mutation: SandboxAclMutation,
    pub readback: Option<SandboxAclReadback>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxLeaseRecoveryView {
    pub lease_id: String,
    pub binding: SandboxLeaseBinding,
    pub acl_mutations: Vec<SandboxAclMutationState>,
    pub process_tree_termination: Option<SandboxProcessTreeTermination>,
    pub appcontainer_removal_intent: bool,
    pub appcontainer_removed: bool,
    pub job_directory_removal_intent: bool,
    pub job_directory_removed: bool,
    pub cleanup_complete: bool,
    pub writeback_receipt: Option<SandboxWritebackReceipt>,
}

/// Verified side-effect digests retained after a successful overlay is removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxWritebackReceipt {
    pub schema_version: p::SchemaVersion,
    pub job_id: String,
    pub plan_digest: p::SchemaDigest,
    pub intent_digest: p::SchemaDigest,
    pub baseline_digest: p::SchemaDigest,
    pub result_digest: p::SchemaDigest,
}

impl SandboxWritebackReceipt {
    fn validate_for(&self, binding: &SandboxLeaseBinding) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
            || self.job_id != binding.job_id
            || self.plan_digest != binding.plan_digest
        {
            return Err(p::Error(
                "workspace receipt is not bound to the original lease".into(),
            ));
        }
        for digest in [
            &self.intent_digest,
            &self.baseline_digest,
            &self.result_digest,
        ] {
            p::validate_product_digest(digest)?;
        }
        Ok(())
    }
}

fn canonical_existing_path(path: &Path) -> p::Result<String> {
    super::reject_linked_ancestors(path)?;
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| p::Error("sandbox target path cannot be resolved".into()))?;
    canonical
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| p::Error("sandbox target path is not valid Unicode".into()))
}

#[cfg(test)]
#[path = "sandbox_lease_tests.rs"]
mod tests;
