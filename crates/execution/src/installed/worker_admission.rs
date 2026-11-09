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
        self.admit_bound(job, permit, execution, None, now)
    }
    pub(in crate::installed) fn admit_with_desktop_output(
        &self,
        job: p::WorkerJob,
        permit: p::WorkerPermit,
        execution: &p::WorkerExecutionSpec,
        transfer: p::DesktopOutputTransfer,
        now: p::Timestamp,
    ) -> p::Result<(bool, p::WorkerObservation)> {
        transfer.validate_intent(&execution.intent)?;
        self.admit_bound(job, permit, Some(execution), Some(transfer), now)
    }
    fn admit_bound(
        &self,
        job: p::WorkerJob,
        permit: p::WorkerPermit,
        execution: Option<&p::WorkerExecutionSpec>,
        output_transfer: Option<p::DesktopOutputTransfer>,
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
            if output_transfer
                .as_ref()
                .is_some_and(|value| record.output_transfer.as_ref() != Some(value))
            {
                return Err(invalid_journal());
            }
            return Ok((false, observation(record, now)));
        }
        let screenshot = execution.is_some_and(|spec| {
            matches!(&spec.intent.parameters, p::ActionParameters::Computer(action)
                if action.operation == p::ComputerOperation::Screenshot)
        });
        if output_transfer.is_none() && screenshot {
            return Err(p::Error(
                "screenshot admission requires its bounded output transfer".into(),
            ));
        }
        let job_id = job.job_id.clone();
        append_fact(
            &mut writer,
            WorkerFact::Admitted {
                job: Box::new(job),
                permit: Box::new(permit),
                at: now,
                execution_binding,
                output_transfer: output_transfer.map(Box::new),
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
