//! Durable Host-authored screenshot admission and verified-result metadata.
use super::*;
#[cfg(test)]
#[path = "worker_output_tests.rs"]
mod tests;

pub(super) fn validate_transfer(
    job: &p::WorkerJob,
    permit: &p::WorkerPermit,
    execution: &WorkerExecutionBinding,
    transfer: &p::DesktopOutputTransfer,
) -> p::Result<()> {
    transfer.validate()?;
    let binding = execution
        .desktop_session
        .as_ref()
        .ok_or_else(invalid_journal)?;
    let request = p::DesktopActionRequest {
        schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
        job_id: job.job_id.clone(),
        permit_ref: permit.permit_ref.clone(),
        binding: binding.clone(),
        normalized_action_digest: execution.normalized_action_digest.clone(),
        capability: p::DesktopCapability::Screenshot,
        deadline: transfer.binding.deadline,
    };
    let expected = p::DesktopExchangeBinding::from_request(
        transfer.binding.installation_ref.clone(),
        job,
        permit,
        &request,
    )?;
    if execution.backend != p::BackendKind::Computer
        || transfer.binding != expected
        || transfer.output_budget_bytes
            != execution
                .output_budget_bytes
                .min(p::DESKTOP_OUTPUT_MAX_BYTES)
    {
        return Err(invalid_journal());
    }
    Ok(())
}
pub(super) fn validate_output(
    record: &WorkerRecord,
    descriptor: &p::DesktopOutputDescriptor,
) -> p::Result<()> {
    let manifest = record
        .result_manifest
        .as_ref()
        .ok_or_else(invalid_journal)?;
    descriptor.validate_for(manifest)?;
    manifest.validate_for(&record.job, &record.permit)?;
    let execution = record
        .execution_binding
        .as_ref()
        .ok_or_else(invalid_journal)?;
    let transfer = record
        .output_transfer
        .as_ref()
        .ok_or_else(invalid_journal)?;
    validate_transfer(&record.job, &record.permit, execution, transfer)?;
    if descriptor.binding != transfer.binding
        || (descriptor.width, descriptor.height) != (transfer.width, transfer.height)
        || execution.output_budget_bytes != manifest.binding.output_budget_bytes
        || record.desktop_binding.as_ref() != execution.desktop_session.as_ref()
        || manifest.bytes > transfer.output_budget_bytes
    {
        return Err(invalid_journal());
    }
    Ok(())
}
pub(super) fn validate_completion(
    record: &WorkerRecord,
    receipt: &p::WorkerReceipt,
) -> p::Result<()> {
    if record.output_transfer.is_none() {
        return Ok(());
    }
    // Output failure can only be Unknown after capture has started. In
    // particular a successful receipt cannot precede descriptor persistence.
    if receipt.observed_outcome != p::WorkerOutcome::Completed {
        return Err(invalid_journal());
    }
    let descriptor = record.desktop_output.as_ref().ok_or_else(invalid_journal)?;
    validate_output(record, descriptor)?;
    let manifest = record
        .result_manifest
        .as_ref()
        .ok_or_else(invalid_journal)?;
    if manifest.binding.receipt_ref.as_ref() != Some(&receipt.receipt_ref)
        || manifest.result_ref != receipt.safe_result_ref
        || receipt
            .finished_at
            .is_none_or(|at| at > descriptor.binding.deadline || at > manifest.produced_at)
        || manifest.evidence_refs != receipt.ground_truth_refs
    {
        return Err(invalid_journal());
    }
    Ok(())
}
impl WorkerJournal {
    pub(in crate::installed) fn record_desktop_output(
        &self,
        descriptor: p::DesktopOutputDescriptor,
    ) -> p::Result<()> {
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        let record = writer
            .projection
            .jobs
            .get(&descriptor.binding.job_id)
            .ok_or_else(invalid_journal)?;
        validate_output(record, &descriptor)?;
        if record.desktop_output.as_ref() == Some(&descriptor) {
            return Ok(());
        }
        if record.desktop_output.is_some() || record.state != p::WorkerState::Running {
            return Err(invalid_journal());
        }
        append_fact(
            &mut writer,
            WorkerFact::DesktopOutput {
                descriptor: Box::new(descriptor),
            },
        )
    }
    pub(in crate::installed) fn desktop_output(
        &self,
        job_id: &str,
    ) -> p::Result<p::DesktopOutputDescriptor> {
        validate_worker_id(job_id)?;
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        if let Err(error) = recovery::recheck_writer(&mut writer) {
            writer.poisoned = true;
            return Err(error);
        }
        let record = writer
            .projection
            .jobs
            .get(job_id)
            .ok_or_else(invalid_journal)?;
        let descriptor = record.desktop_output.clone().ok_or_else(invalid_journal)?;
        validate_output(record, &descriptor)?;
        Ok(descriptor)
    }
}
