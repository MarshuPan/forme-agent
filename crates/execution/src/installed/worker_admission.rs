use super::*;

impl WorkerJournal {
    pub fn admit(
        &self,
        job: p::WorkerJob,
        permit: p::WorkerPermit,
        now: p::Timestamp,
    ) -> p::Result<(bool, p::WorkerObservation)> {
        self.admit_with_execution(job, permit, None, now)
    }

    pub fn admit_with_execution(
        &self,
        job: p::WorkerJob,
        permit: p::WorkerPermit,
        execution: Option<&p::WorkerExecutionSpec>,
        now: p::Timestamp,
    ) -> p::Result<(bool, p::WorkerObservation)> {
        job.validate()?;
        permit.validate_for(&job, now)?;
        let execution_binding = execution.map(WorkerExecutionBinding::from_spec);
        if let Some(binding) = &execution_binding {
            binding.validate_for(&job, &permit, now)?;
        }
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        if let Some(record) = writer.projection.jobs.get(&job.job_id) {
            if record.job != job
                || record.permit != permit
                || record.execution_binding != execution_binding
            {
                return Err(p::Error(
                    "worker job id conflicts with its original admission".into(),
                ));
            }
            return Ok((false, observation(record, now)));
        }
        let job_id = job.job_id.clone();
        append_fact(
            &mut writer,
            WorkerFact::Admitted {
                job: Box::new(job),
                permit: Box::new(permit),
                at: now,
                execution_binding,
            },
        )?;
        let record = writer
            .projection
            .jobs
            .get(&job_id)
            .ok_or_else(invalid_journal)?;
        Ok((true, observation(record, now)))
    }
}
