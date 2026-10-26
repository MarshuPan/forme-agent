//! Bounded, immutable worker result protocol objects.

use super::worker::{
    validate_worker_action, validate_worker_nonce, validate_worker_plan_digest,
    validate_worker_principal, validate_worker_ref, validate_workspace_scope, WorkerJob,
    WorkerOutcome, WorkerPermit,
};
use super::{validate_product_digest, version};
use crate::{
    canonical_digest, ActionId, AuthorityEpoch, Error, Nonce, PlanDigest, Result, SchemaDigest,
    SchemaVersion, Scope, Timestamp, VerifiedPrincipal,
};
use serde::{Deserialize, Serialize};

pub const WORKER_RESULT_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);
pub const WORKER_RESULT_MAX_CHUNK_BYTES: u64 = 256 * 1024;
pub const WORKER_RESULT_MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerResultStatus {
    Available,
    Empty,
    Pending,
    Missing,
    Corrupt,
    Cleaned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerResultEncoding {
    Binary,
    Utf8,
}

/// The immutable binding shared by a worker result manifest and every read.
/// The installation reference is derived from the protected AgentExecution
/// credential by the Host; it is not accepted from a worker or Web request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerResultBinding {
    pub schema_version: SchemaVersion,
    pub installation_ref: String,
    pub job_id: String,
    pub owner_ref: VerifiedPrincipal,
    pub action_ref: ActionId,
    pub workspace_scope: Scope,
    pub plan_digest: PlanDigest,
    #[serde(default)]
    pub normalized_action_digest: Option<SchemaDigest>,
    pub permit_ref: String,
    pub permit_digest: SchemaDigest,
    pub authority_epoch: AuthorityEpoch,
    pub nonce: Nonce,
    #[serde(default)]
    pub receipt_ref: Option<String>,
    pub output_budget_bytes: u64,
}

impl WorkerResultBinding {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_worker_ref(&self.installation_ref, "worker installation")?;
        validate_worker_ref(&self.job_id, "worker job")?;
        validate_worker_principal(&self.owner_ref)?;
        validate_worker_action(&self.action_ref)?;
        validate_workspace_scope(&self.workspace_scope)?;
        validate_worker_plan_digest(&self.plan_digest)?;
        if let Some(digest) = &self.normalized_action_digest {
            validate_product_digest(digest)?;
        }
        validate_worker_ref(&self.permit_ref, "worker permit")?;
        validate_product_digest(&self.permit_digest)?;
        if self.authority_epoch.0 == 0 {
            return Err(Error("worker result authority epoch is invalid".into()));
        }
        validate_worker_nonce(&self.nonce)?;
        if let Some(receipt_ref) = &self.receipt_ref {
            validate_worker_ref(receipt_ref, "worker result receipt")?;
        }
        if self.output_budget_bytes == 0 || self.output_budget_bytes > WORKER_RESULT_MAX_BYTES {
            return Err(Error("worker result output budget is invalid".into()));
        }
        Ok(())
    }

    pub fn validate_for(&self, job: &WorkerJob, permit: &WorkerPermit) -> Result<()> {
        self.validate()?;
        job.validate()?;
        permit.validate()?;
        let expected_permit_digest = canonical_digest(permit)?;
        if self.job_id != job.job_id
            || self.owner_ref != job.owner_ref
            || self.action_ref != job.action_ref
            || self.workspace_scope != job.workspace_scope
            || self.plan_digest != job.plan_digest
            || self.permit_ref != permit.permit_ref
            || self.permit_digest != expected_permit_digest
            || self.authority_epoch != job.authority_epoch
            || self.nonce != job.nonce
        {
            return Err(Error(
                "worker result binding is not bound to the exact job".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerResultManifest {
    pub schema_version: SchemaVersion,
    pub manifest_digest: SchemaDigest,
    pub binding: WorkerResultBinding,
    #[serde(default)]
    pub result_ref: Option<String>,
    pub status: WorkerResultStatus,
    #[serde(default)]
    pub outcome: Option<WorkerOutcome>,
    pub bytes: u64,
    #[serde(default)]
    pub content_digest: Option<SchemaDigest>,
    pub encoding: WorkerResultEncoding,
    pub truncated: bool,
    pub produced_at: Timestamp,
    pub evidence_refs: Vec<String>,
}

impl WorkerResultManifest {
    pub fn expected_digest(&self) -> Result<SchemaDigest> {
        let mut material = self.clone();
        material.manifest_digest = SchemaDigest(String::new());
        canonical_digest(&material)
    }

    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        self.binding.validate()?;
        validate_product_digest(&self.manifest_digest)?;
        if self.expected_digest()? != self.manifest_digest {
            return Err(Error("worker result manifest digest is invalid".into()));
        }
        if self.produced_at <= 0 || self.bytes > WORKER_RESULT_MAX_BYTES || self.truncated {
            return Err(Error("worker result manifest bounds are invalid".into()));
        }
        if let Some(result_ref) = &self.result_ref {
            validate_worker_ref(result_ref, "worker result")?;
        }
        if let Some(digest) = &self.content_digest {
            validate_product_digest(digest)?;
        }
        if self.evidence_refs.len() > 128 {
            return Err(Error(
                "worker result evidence references are unbounded".into(),
            ));
        }
        for reference in &self.evidence_refs {
            validate_worker_ref(reference, "worker result evidence reference")?;
        }
        match self.status {
            WorkerResultStatus::Available => {
                if self.outcome.is_none()
                    || self.bytes == 0
                    || self.result_ref.is_none()
                    || self.content_digest.is_none()
                {
                    return Err(Error("available worker result is incomplete".into()));
                }
            }
            WorkerResultStatus::Empty => {
                if self.outcome.is_none()
                    || self.bytes != 0
                    || self.result_ref.is_none()
                    || self.content_digest.is_none()
                {
                    return Err(Error("empty worker result is incomplete".into()));
                }
            }
            WorkerResultStatus::Pending => {
                if self.outcome.is_some()
                    || self.bytes != 0
                    || self.result_ref.is_some()
                    || self.content_digest.is_some()
                {
                    return Err(Error("pending worker result has content".into()));
                }
            }
            WorkerResultStatus::Missing
            | WorkerResultStatus::Corrupt
            | WorkerResultStatus::Cleaned => {
                if self.outcome.is_none() || self.bytes != 0 || self.content_digest.is_some() {
                    return Err(Error("unavailable worker result has content".into()));
                }
            }
        }
        Ok(())
    }

    pub fn validate_for(&self, job: &WorkerJob, permit: &WorkerPermit) -> Result<()> {
        self.validate()?;
        self.binding.validate_for(job, permit)?;
        if let Some(receipt_ref) = &self.binding.receipt_ref {
            validate_worker_ref(receipt_ref, "worker result receipt")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerResultRead {
    pub schema_version: SchemaVersion,
    pub binding: WorkerResultBinding,
    pub manifest_digest: SchemaDigest,
    pub offset: u64,
    pub max_bytes: u64,
}

impl WorkerResultRead {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        self.binding.validate()?;
        validate_product_digest(&self.manifest_digest)?;
        if self.max_bytes == 0 || self.max_bytes > WORKER_RESULT_MAX_CHUNK_BYTES {
            return Err(Error("worker result read size is invalid".into()));
        }
        Ok(())
    }

    pub fn validate_for(&self, manifest: &WorkerResultManifest) -> Result<()> {
        self.validate()?;
        if self.manifest_digest != manifest.manifest_digest || self.binding != manifest.binding {
            return Err(Error(
                "worker result read is not bound to the manifest".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerResultChunk {
    pub schema_version: SchemaVersion,
    pub manifest_digest: SchemaDigest,
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub chunk_digest: SchemaDigest,
    pub next_offset: u64,
    pub finished: bool,
}

impl WorkerResultChunk {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_product_digest(&self.manifest_digest)?;
        if self.bytes.len() as u64 > WORKER_RESULT_MAX_CHUNK_BYTES
            || self.next_offset != self.offset + self.bytes.len() as u64
        {
            return Err(Error("worker result chunk bounds are invalid".into()));
        }
        let expected = crate::sha256_content_digest(&self.bytes);
        if self.chunk_digest != expected {
            return Err(Error("worker result chunk digest is invalid".into()));
        }
        Ok(())
    }
}
