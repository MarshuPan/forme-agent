//! Read-only original job and result bindings.
use super::*;

impl WorkerJournal {
    pub fn job(&self, job_id: &str) -> p::Result<p::WorkerJob> {
        validate_worker_id(job_id)?;
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        writer
            .projection
            .jobs
            .get(job_id)
            .map(|record| record.job.clone())
            .ok_or_else(|| p::Error("worker job does not exist".into()))
    }

    pub fn result_manifest(&self, job_id: &str) -> p::Result<Option<p::WorkerResultManifest>> {
        validate_worker_id(job_id)?;
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        Ok(writer
            .projection
            .jobs
            .get(job_id)
            .and_then(|record| record.result_manifest.clone()))
    }

    pub fn result_binding(
        &self,
        job_id: &str,
        installation_ref: String,
        receipt_ref: Option<String>,
    ) -> p::Result<p::WorkerResultBinding> {
        validate_worker_id(job_id)?;
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        let record = writer
            .projection
            .jobs
            .get(job_id)
            .ok_or_else(|| p::Error("worker job does not exist".into()))?;
        let (normalized_action_digest, output_budget_bytes) = record
            .execution_binding
            .as_ref()
            .map(|binding| {
                (
                    Some(binding.normalized_action_digest.clone()),
                    binding.output_budget_bytes,
                )
            })
            .unwrap_or((None, p::WORKER_RESULT_MAX_BYTES));
        let binding = p::WorkerResultBinding {
            schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
            installation_ref,
            job_id: record.job.job_id.clone(),
            owner_ref: record.job.owner_ref.clone(),
            action_ref: record.job.action_ref.clone(),
            workspace_scope: record.job.workspace_scope.clone(),
            plan_digest: record.job.plan_digest.clone(),
            normalized_action_digest,
            permit_ref: record.permit.permit_ref.clone(),
            permit_digest: p::canonical_digest(&record.permit)?,
            authority_epoch: record.job.authority_epoch,
            nonce: record.job.nonce.clone(),
            receipt_ref,
            output_budget_bytes,
        };
        binding.validate()?;
        Ok(binding)
    }
}
