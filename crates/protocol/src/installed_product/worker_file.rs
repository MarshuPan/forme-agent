//! Host-derived mapping from an immutable logical action to its private copy.
use serde::{Deserialize, Serialize};

use super::*;
use crate::PlanDigest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerFileTarget {
    pub schema_version: SchemaVersion,
    pub job_id: String,
    pub plan_digest: PlanDigest,
    pub normalized_action_digest: SchemaDigest,
    pub relative_path: String,
}

impl WorkerFileTarget {
    /// This is a read-only Host input, never an admission or an alternate plan.
    pub fn validate_for(&self, job: &WorkerJob, execution: &WorkerExecutionSpec) -> Result<()> {
        version(self.schema_version)?;
        validate_product_id(&self.job_id)?;
        validate_product_digest(&SchemaDigest(self.plan_digest.0.clone()))?;
        validate_product_digest(&self.normalized_action_digest)?;
        crate::validate_release_relative_path(&self.relative_path)?;
        if self.job_id != job.job_id
            || self.plan_digest != job.plan_digest
            || self.plan_digest != execution.plan_digest
            || self.normalized_action_digest != execution.normalized_action_digest
            || execution.backend != crate::BackendKind::File
            || !matches!(
                execution.intent.parameters,
                crate::ActionParameters::File { .. }
            )
        {
            return Err(Error(
                "worker file mapping is not bound to the original action".into(),
            ));
        }
        Ok(())
    }
}
