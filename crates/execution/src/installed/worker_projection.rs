//! Worker journal replay with original versioned hash material.
use super::*;

impl WorkerProjection {
    pub(super) fn head(&self) -> p::Result<p::SchemaDigest> {
        self.head
            .clone()
            .map_or_else(|| p::canonical_digest(&"forme-worker-journal-v1"), Ok)
    }

    pub(super) fn fold(&mut self, envelope: &WorkerEnvelope) -> p::Result<()> {
        if !matches!(envelope.schema_version.0, 1 | 2)
            || (self.uses_output && envelope.schema_version != p::SchemaVersion(2))
            || envelope.worker_seq != self.sequence.checked_add(1).ok_or_else(invalid_journal)?
            || envelope.previous != self.head()?
            || envelope.digest
                != WorkerEnvelope::calculate(
                    envelope.schema_version,
                    envelope.worker_seq,
                    &envelope.previous,
                    &envelope.fact,
                )?
        {
            return Err(invalid_journal());
        }

        match &envelope.fact {
            WorkerFact::Admitted {
                job,
                permit,
                at,
                execution_binding,
                output_transfer,
            } => {
                job.validate()?;
                permit.validate_for(job, *at)?;
                if let Some(binding) = execution_binding {
                    binding.validate_for(job, permit, *at)?;
                }
                if let Some(transfer) = output_transfer {
                    if envelope.schema_version != p::SchemaVersion(2) {
                        return Err(invalid_journal());
                    }
                    output::validate_transfer(
                        job,
                        permit,
                        execution_binding.as_ref().ok_or_else(invalid_journal)?,
                        transfer,
                    )?;
                }
                if self.jobs.contains_key(&job.job_id)
                    || !self.nonces.insert(job.nonce.clone())
                    || !self.permits.insert(job.permit_ref.clone())
                {
                    return Err(invalid_journal());
                }
                self.jobs.insert(
                    job.job_id.clone(),
                    WorkerRecord {
                        #[cfg(windows)]
                        admitted_seq: envelope.worker_seq,
                        #[cfg(windows)]
                        admitted_at: *at,
                        job: *job.clone(),
                        permit: *permit.clone(),
                        execution_binding: execution_binding.clone(),
                        desktop_binding: None,
                        desktop_observation: None,
                        state: p::WorkerState::Pending,
                        receipt: None,
                        result_manifest: None,
                        output_transfer: output_transfer.as_deref().cloned(),
                        desktop_output: None,
                        cancel_receipt: None,
                        reason_ref: None,
                    },
                );
            }
            WorkerFact::DesktopObservation {
                job_id,
                binding,
                observation,
            } => {
                validate_worker_id(job_id)?;
                let record = self.jobs.get_mut(job_id).ok_or_else(invalid_journal)?;
                if !matches!(
                    record.state,
                    p::WorkerState::Pending | p::WorkerState::Running
                ) {
                    return Err(invalid_journal());
                }
                binding.validate_for(&record.job, &record.permit, observation.observed_at)?;
                observation.validate()?;
                if observation.state == p::DesktopSessionState::Ready {
                    observation.validate_for(binding)?;
                } else if observation.helper_ref != binding.helper_ref
                    || observation.platform != binding.platform
                {
                    return Err(invalid_journal());
                }
                record.desktop_binding = Some(*binding.clone());
                record.desktop_observation = Some(*observation.clone());
            }
            WorkerFact::Started { job_id, at } => {
                validate_worker_id(job_id)?;
                let record = self.jobs.get_mut(job_id).ok_or_else(invalid_journal)?;
                if *at <= 0 || record.state != p::WorkerState::Pending {
                    return Err(invalid_journal());
                }
                record.state = p::WorkerState::Running;
            }
            WorkerFact::CancelRequested { job_id, at } => {
                validate_worker_id(job_id)?;
                let record = self.jobs.get_mut(job_id).ok_or_else(invalid_journal)?;
                if *at <= 0
                    || !matches!(
                        record.state,
                        p::WorkerState::Pending | p::WorkerState::Running
                    )
                {
                    return Err(invalid_journal());
                }
                record.state = p::WorkerState::CancelRequested;
            }
            WorkerFact::Receipt { receipt } => {
                let record = self
                    .jobs
                    .get_mut(&receipt.job_id)
                    .ok_or_else(invalid_journal)?;
                receipt.validate_for(&record.job)?;
                if !matches!(
                    record.state,
                    p::WorkerState::Running | p::WorkerState::CancelRequested
                ) || receipt.finished_at.is_none()
                {
                    return Err(invalid_journal());
                }
                output::validate_completion(record, receipt)?;
                record.state = match receipt.observed_outcome {
                    p::WorkerOutcome::Completed => p::WorkerState::Completed,
                    p::WorkerOutcome::Failed => p::WorkerState::Failed,
                    p::WorkerOutcome::Cancelled => p::WorkerState::Cancelled,
                    p::WorkerOutcome::Unknown => p::WorkerState::Unknown,
                };
                record.receipt = Some(receipt.clone());
                record.reason_ref = None;
            }
            WorkerFact::CancelReceipt { receipt } => {
                let record = self
                    .jobs
                    .get_mut(&receipt.job_id)
                    .ok_or_else(invalid_journal)?;
                receipt.validate_for(&record.job)?;
                if !matches!(
                    record.state,
                    p::WorkerState::Pending
                        | p::WorkerState::Running
                        | p::WorkerState::CancelRequested
                ) {
                    return Err(invalid_journal());
                }
                record.state = match receipt.outcome {
                    p::WorkerOutcome::Cancelled => p::WorkerState::Cancelled,
                    p::WorkerOutcome::Unknown => p::WorkerState::Unknown,
                    _ => return Err(invalid_journal()),
                };
                record.reason_ref = receipt.reason_ref.clone();
                record.cancel_receipt = Some(receipt.clone());
            }
            WorkerFact::DesktopOutput { descriptor } => {
                let record = self
                    .jobs
                    .get_mut(&descriptor.binding.job_id)
                    .ok_or_else(invalid_journal)?;
                if envelope.schema_version != p::SchemaVersion(2)
                    || record.desktop_output.is_some()
                    || record.state != p::WorkerState::Running
                {
                    return Err(invalid_journal());
                }
                output::validate_output(record, descriptor)?;
                record.desktop_output = Some(*descriptor.clone());
            }
            WorkerFact::ResultManifest { manifest } => {
                let record = self
                    .jobs
                    .get_mut(&manifest.binding.job_id)
                    .ok_or_else(invalid_journal)?;
                manifest.validate_for(&record.job, &record.permit)?;
                if !matches!(
                    record.state,
                    p::WorkerState::Pending
                        | p::WorkerState::Running
                        | p::WorkerState::CancelRequested
                ) || record.result_manifest.is_some()
                {
                    return Err(invalid_journal());
                }
                record.result_manifest = Some(manifest.clone());
            }
            WorkerFact::Unknown {
                job_id,
                reason_ref,
                at,
            } => {
                validate_worker_id(job_id)?;
                validate_worker_ref(reason_ref, "worker unknown reason")?;
                let record = self.jobs.get_mut(job_id).ok_or_else(invalid_journal)?;
                if *at <= 0
                    || !matches!(
                        record.state,
                        p::WorkerState::Pending
                            | p::WorkerState::Running
                            | p::WorkerState::CancelRequested
                    )
                {
                    return Err(invalid_journal());
                }
                record.state = p::WorkerState::Unknown;
                record.reason_ref = Some(reason_ref.clone());
            }
        }
        self.uses_output |= envelope.schema_version == p::SchemaVersion(2);
        self.sequence = envelope.worker_seq;
        self.head = Some(envelope.digest.clone());
        Ok(())
    }
}
