//! Windows per-job copy and exact-plan-bound Host writeback.
#![cfg(windows)]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use forme_protocol as p;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::process::write_private_json;
use super::windows_appcontainer_acl as acl;

mod authorization;
mod custody;
mod transaction;

use authorization::WriteScope;
use custody::{unavailable, Entry, Tree};

const MAX_ENTRIES: usize = 1024;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RECORD_BYTES: u64 = 4 * 1024 * 1024;
const BASELINE: &str = "overlay-baseline.json";
const INTENT: &str = "overlay-writeback-intent.json";
const RECEIPT: &str = "overlay-writeback-receipt.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Baseline {
    schema_version: p::SchemaVersion,
    job: p::WorkerJob,
    permit: p::WorkerPermit,
    execution: p::WorkerExecutionSpec,
    original_root: PathBuf,
    original_identity: super::sandbox_lease::SandboxFileIdentity,
    overlay_identity: super::sandbox_lease::SandboxFileIdentity,
    entries: BTreeMap<String, Entry>,
}

pub(super) struct PreparedOverlay {
    run_dir: PathBuf,
    baseline: Baseline,
    scope: WriteScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Completion {
    Committed,
    Rejected,
}

impl PreparedOverlay {
    pub fn prepare(
        run_dir: &Path,
        root: &Path,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        execution: Option<&p::WorkerExecutionSpec>,
        now: p::Timestamp,
    ) -> p::Result<Self> {
        permit.validate_for(job, now)?;
        let execution = execution
            .ok_or_else(|| p::Error("private workspace requires an exact execution spec".into()))?;
        execution.validate_for(job, permit, now)?;
        let scope = WriteScope::derive(root, execution)?;
        let source = Tree::open(root, &Default::default())?;
        let workspace = run_dir.join("workspace");
        source.copy_to(&workspace)?;
        let baseline = Baseline {
            schema_version: p::SchemaVersion(1),
            job: job.clone(),
            permit: permit.clone(),
            execution: execution.clone(),
            original_root: root.to_path_buf(),
            original_identity: source.identity.clone(),
            overlay_identity: acl::file_identity(&workspace)?,
            entries: source.entries.clone(),
        };
        write_record(&run_dir.join(BASELINE), &baseline)?;
        Ok(Self {
            run_dir: run_dir.to_path_buf(),
            baseline,
            scope,
        })
    }

    pub fn workspace(&self) -> PathBuf {
        self.run_dir.join("workspace")
    }

    pub fn file_target(&self) -> p::Result<Option<p::WorkerFileTarget>> {
        let WriteScope::File { path, .. } = &self.scope else {
            return Ok(None);
        };
        let target = p::WorkerFileTarget {
            schema_version: p::SchemaVersion(1),
            job_id: self.baseline.job.job_id.clone(),
            plan_digest: self.baseline.job.plan_digest.clone(),
            normalized_action_digest: self.baseline.execution.normalized_action_digest.clone(),
            relative_path: path.clone(),
        };
        target.validate_for(&self.baseline.job, &self.baseline.execution)?;
        Ok(Some(target))
    }

    /// Change labels only below the newly-created per-job copy, never the
    /// original workspace, input records or Host intent/receipt directory.
    pub fn prepare_low_integrity(&self) -> p::Result<()> {
        let workspace = self.workspace();
        acl::set_low_integrity(&workspace)?;
        for key in self.baseline.entries.keys() {
            acl::set_low_integrity(&workspace.join(key))?;
        }
        Ok(())
    }

    pub fn complete(&self, job: &p::WorkerJob, now: p::Timestamp) -> p::Result<Completion> {
        // A fresh attempt must never overwrite a previous intent, even if a
        // prior invocation failed before the first file write.
        if self
            .run_dir
            .join(INTENT)
            .try_exists()
            .map_err(|_| unavailable())?
        {
            return Err(p::Error(
                "workspace writeback already has an intent; original outcome needs resolution"
                    .into(),
            ));
        }
        let prepared = transaction::preflight(self, job, now);
        let Ok(prepared) = prepared else {
            return Ok(Completion::Rejected);
        };
        transaction::apply(self, prepared, now)?;
        Ok(Completion::Committed)
    }
}

/// Startup must retain unknown writeback evidence. This does not replay an
/// intent or infer success from the current contents of the original tree.
pub(super) fn verify_cleanup_allowed(
    run_dir: &Path,
) -> p::Result<Option<super::sandbox_lease::SandboxWritebackReceipt>> {
    let intent_path = run_dir.join(INTENT);
    let receipt_path = run_dir.join(RECEIPT);
    let has_intent = intent_path.try_exists().map_err(|_| unavailable())?;
    let has_receipt = receipt_path.try_exists().map_err(|_| unavailable())?;
    if !has_intent && !has_receipt {
        return Ok(None);
    }
    if !has_intent || !has_receipt {
        return Err(p::Error(
            "workspace writeback is unknown; preserve overlay and resolve the original job".into(),
        ));
    }
    let baseline: Baseline = read_record(&run_dir.join(BASELINE))?;
    let intent: transaction::Intent = read_record(&intent_path)?;
    let receipt: transaction::Receipt = read_record(&receipt_path)?;
    transaction::validate_receipt(&baseline, &intent, &receipt)?;
    Ok(Some(transaction::retained_receipt(&baseline, &receipt)))
}

fn write_record<T: Serialize>(path: &Path, value: &T) -> p::Result<()> {
    if serde_json::to_vec(value).map_err(|_| unavailable())?.len() as u64 > MAX_RECORD_BYTES {
        return Err(unavailable());
    }
    write_private_json(path, value)
}

fn read_record<T: DeserializeOwned>(path: &Path) -> p::Result<T> {
    super::reject_linked_ancestors(path)?;
    let file = File::open(path).map_err(|_| unavailable())?;
    let (_, links) = acl::inspect_file(&file)?;
    if links != 1 || file.metadata().map_err(|_| unavailable())?.len() > MAX_RECORD_BYTES {
        return Err(unavailable());
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unavailable())?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(unavailable());
    }
    serde_json::from_slice(&bytes).map_err(|_| unavailable())
}

#[cfg(test)]
mod tests;
