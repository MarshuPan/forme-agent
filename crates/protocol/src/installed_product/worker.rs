//! Worker admission DTOs and validation, kept separate from product lifecycle
//! and release contracts so changes to execution do not churn installation
//! protocol code.
use super::{validate_product_digest, validate_product_id, version};
use crate::{
    ActionId, AuthorityEpoch, Error, Nonce, PlanDigest, ResourceRef, Result, SchemaDigest,
    SchemaVersion, Scope, Timestamp, VerifiedPrincipal,
};
use serde::{Deserialize, Serialize};

/// A product-side worker state. This is deliberately outside the brain event
/// taxonomy: it describes an execution host fact, not cognition or a Run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerState {
    Pending,
    Running,
    CancelRequested,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerOutcome {
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

fn validate_worker_ref(value: &str, name: &str) -> Result<()> {
    validate_product_id(value).map_err(|_| Error(format!("{name} is invalid")))
}

fn validate_worker_plan_digest(value: &PlanDigest) -> Result<()> {
    validate_product_digest(&SchemaDigest(value.0.clone()))
}

fn validate_workspace_scope(value: &Scope) -> Result<()> {
    if value.0.len() > 512
        || !value.0.starts_with("workspace:")
        || value.0.contains(['\\', '\0'])
        || value.0.chars().any(char::is_control)
        || value
            .0
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error("worker workspace scope is invalid".into()));
    }
    Ok(())
}

fn validate_worker_nonce(value: &Nonce) -> Result<()> {
    validate_worker_ref(&value.0, "worker nonce")
}

fn validate_worker_principal(value: &VerifiedPrincipal) -> Result<()> {
    validate_worker_ref(&value.0, "worker owner")
}

fn validate_worker_action(value: &ActionId) -> Result<()> {
    validate_worker_ref(&value.0, "worker action")
}

fn validate_worker_resource(value: &ResourceRef) -> Result<()> {
    if value.0.is_empty()
        || value.0.len() > 512
        || value.0.contains(['\\', '\0'])
        || value.0.chars().any(char::is_control)
        || value.0.split('/').any(|part| part == "." || part == "..")
    {
        return Err(Error("worker resource reference is invalid".into()));
    }
    Ok(())
}

/// The exact execution admission sent by the core to the product host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerJob {
    pub schema_version: SchemaVersion,
    pub job_id: String,
    pub owner_ref: VerifiedPrincipal,
    pub action_ref: ActionId,
    pub affected_resources: Vec<ResourceRef>,
    pub execution_plan_ref: String,
    pub plan_digest: PlanDigest,
    pub authority_epoch: AuthorityEpoch,
    pub permit_ref: String,
    pub sandbox_profile_ref: String,
    pub workspace_scope: Scope,
    pub deadline: Timestamp,
    pub nonce: Nonce,
    pub cancel_ref: String,
}

impl WorkerJob {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_worker_ref(&self.job_id, "worker job")?;
        validate_worker_principal(&self.owner_ref)?;
        validate_worker_action(&self.action_ref)?;
        if self.affected_resources.is_empty() || self.affected_resources.len() > 256 {
            return Err(Error(
                "worker affected resources are missing or unbounded".into(),
            ));
        }
        for resource in &self.affected_resources {
            validate_worker_resource(resource)?;
        }
        validate_worker_ref(&self.execution_plan_ref, "worker execution plan")?;
        validate_worker_plan_digest(&self.plan_digest)?;
        if self.authority_epoch.0 == 0 {
            return Err(Error("worker authority epoch is invalid".into()));
        }
        validate_worker_ref(&self.permit_ref, "worker permit")?;
        validate_worker_ref(&self.sandbox_profile_ref, "worker sandbox profile")?;
        validate_workspace_scope(&self.workspace_scope)?;
        if self.deadline <= 0 {
            return Err(Error("worker deadline is invalid".into()));
        }
        validate_worker_nonce(&self.nonce)?;
        validate_worker_ref(&self.cancel_ref, "worker cancel reference")
    }
}

/// The authority-bound admission proof. The host never derives a new permit
/// from a job; it only accepts an exact permit supplied by the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerPermit {
    pub schema_version: SchemaVersion,
    pub permit_ref: String,
    pub job_id: String,
    pub owner_ref: VerifiedPrincipal,
    pub action_ref: ActionId,
    pub affected_resources: Vec<ResourceRef>,
    pub plan_digest: PlanDigest,
    pub authority_epoch: AuthorityEpoch,
    pub workspace_scope: Scope,
    pub sandbox_profile_ref: String,
    pub deadline: Timestamp,
    pub nonce: Nonce,
    pub issued_at: Timestamp,
}

impl WorkerPermit {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_worker_ref(&self.permit_ref, "worker permit")?;
        validate_worker_ref(&self.job_id, "worker job")?;
        validate_worker_principal(&self.owner_ref)?;
        validate_worker_action(&self.action_ref)?;
        if self.affected_resources.is_empty() || self.affected_resources.len() > 256 {
            return Err(Error(
                "worker permit resources are missing or unbounded".into(),
            ));
        }
        for resource in &self.affected_resources {
            validate_worker_resource(resource)?;
        }
        validate_worker_plan_digest(&self.plan_digest)?;
        if self.authority_epoch.0 == 0 {
            return Err(Error("worker permit authority epoch is invalid".into()));
        }
        validate_workspace_scope(&self.workspace_scope)?;
        validate_worker_ref(&self.sandbox_profile_ref, "worker sandbox profile")?;
        if self.issued_at <= 0 || self.deadline <= self.issued_at {
            return Err(Error("worker permit lifetime is invalid".into()));
        }
        validate_worker_nonce(&self.nonce)
    }

    pub fn validate_for(&self, job: &WorkerJob, now: Timestamp) -> Result<()> {
        self.validate()?;
        job.validate()?;
        if self.permit_ref != job.permit_ref
            || self.job_id != job.job_id
            || self.owner_ref != job.owner_ref
            || self.action_ref != job.action_ref
            || self.affected_resources != job.affected_resources
            || self.plan_digest != job.plan_digest
            || self.authority_epoch != job.authority_epoch
            || self.workspace_scope != job.workspace_scope
            || self.sandbox_profile_ref != job.sandbox_profile_ref
            || self.deadline != job.deadline
            || self.nonce != job.nonce
        {
            return Err(Error("worker permit is not bound to the exact job".into()));
        }
        if now >= self.deadline {
            return Err(Error("worker permit has expired".into()));
        }
        Ok(())
    }
}

/// The only wire request that can admit a worker. The execution host accepts
/// the exact job and permit together; it never derives a permit from a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerDispatchRequest {
    pub schema_version: SchemaVersion,
    pub job: WorkerJob,
    pub permit: WorkerPermit,
}

impl WorkerDispatchRequest {
    pub fn validate_at(&self, now: Timestamp) -> Result<()> {
        version(self.schema_version)?;
        self.permit.validate_for(&self.job, now)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerReceipt {
    pub schema_version: SchemaVersion,
    pub receipt_ref: String,
    pub job_id: String,
    pub plan_digest: PlanDigest,
    pub authority_epoch: AuthorityEpoch,
    pub nonce: Nonce,
    pub observed_outcome: WorkerOutcome,
    pub safe_result_ref: Option<String>,
    pub ground_truth_refs: Vec<String>,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
}

impl WorkerReceipt {
    pub fn validate_for(&self, job: &WorkerJob) -> Result<()> {
        version(self.schema_version)?;
        validate_worker_ref(&self.receipt_ref, "worker receipt")?;
        job.validate()?;
        if self.job_id != job.job_id
            || self.plan_digest != job.plan_digest
            || self.authority_epoch != job.authority_epoch
            || self.nonce != job.nonce
        {
            return Err(Error("worker receipt is not bound to the exact job".into()));
        }
        if self.started_at <= 0
            || self
                .finished_at
                .is_some_and(|finished| finished < self.started_at)
        {
            return Err(Error("worker receipt timestamps are invalid".into()));
        }
        if let Some(reference) = &self.safe_result_ref {
            validate_worker_ref(reference, "worker result reference")?;
        }
        if self.ground_truth_refs.len() > 128 {
            return Err(Error("worker ground-truth references are unbounded".into()));
        }
        for reference in &self.ground_truth_refs {
            validate_worker_ref(reference, "worker ground-truth reference")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerObservation {
    pub schema_version: SchemaVersion,
    pub job_id: String,
    pub plan_digest: PlanDigest,
    pub authority_epoch: AuthorityEpoch,
    pub nonce: Nonce,
    pub state: WorkerState,
    pub outcome: Option<WorkerOutcome>,
    pub receipt_ref: Option<String>,
    pub safe_result_ref: Option<String>,
    pub ground_truth_refs: Vec<String>,
    pub observed_at: Timestamp,
    pub reason_ref: Option<String>,
}

impl WorkerObservation {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_worker_ref(&self.job_id, "worker job")?;
        validate_worker_plan_digest(&self.plan_digest)?;
        if self.authority_epoch.0 == 0 {
            return Err(Error(
                "worker observation authority epoch is invalid".into(),
            ));
        }
        validate_worker_nonce(&self.nonce)?;
        if self.observed_at <= 0 {
            return Err(Error("worker observation timestamp is invalid".into()));
        }
        match (self.state, self.outcome) {
            (WorkerState::Pending | WorkerState::Running | WorkerState::CancelRequested, None) => {}
            (WorkerState::Completed, Some(WorkerOutcome::Completed))
            | (WorkerState::Failed, Some(WorkerOutcome::Failed))
            | (WorkerState::Cancelled, Some(WorkerOutcome::Cancelled))
            | (WorkerState::Unknown, Some(WorkerOutcome::Unknown)) => {}
            _ => {
                return Err(Error(
                    "worker observation state and outcome disagree".into(),
                ))
            }
        }
        for reference in self
            .receipt_ref
            .iter()
            .chain(self.safe_result_ref.iter())
            .chain(self.reason_ref.iter())
        {
            validate_worker_ref(reference, "worker observation reference")?;
        }
        if self.ground_truth_refs.len() > 128 {
            return Err(Error("worker observation references are unbounded".into()));
        }
        for reference in &self.ground_truth_refs {
            validate_worker_ref(reference, "worker observation ground truth")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerCancelReceipt {
    pub schema_version: SchemaVersion,
    pub job_id: String,
    pub plan_digest: PlanDigest,
    pub nonce: Nonce,
    pub outcome: WorkerOutcome,
    pub observed_at: Timestamp,
    pub receipt_ref: Option<String>,
    pub reason_ref: Option<String>,
}

impl WorkerCancelReceipt {
    pub fn validate_for(&self, job: &WorkerJob) -> Result<()> {
        version(self.schema_version)?;
        job.validate()?;
        if self.job_id != job.job_id
            || self.plan_digest != job.plan_digest
            || self.nonce != job.nonce
        {
            return Err(Error(
                "worker cancel receipt is not bound to the job".into(),
            ));
        }
        if self.observed_at <= 0 {
            return Err(Error("worker cancel receipt timestamp is invalid".into()));
        }
        for reference in self.receipt_ref.iter().chain(self.reason_ref.iter()) {
            validate_worker_ref(reference, "worker cancel reference")?;
        }
        if !matches!(
            self.outcome,
            WorkerOutcome::Cancelled | WorkerOutcome::Unknown
        ) {
            return Err(Error("worker cancel outcome is invalid".into()));
        }
        Ok(())
    }
}
