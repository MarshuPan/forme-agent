//! Exact helper preparation and dispatch under the existing worker exclusion.
use super::*;

impl<I: WorkerIsolation, D: DesktopHelperAdapter> WorkerExecutorHost<I, D> {
    pub(super) fn dispatch_authorized(
        &self,
        auth: &HostAuth,
        request: p::WorkerDispatchRequest,
        now: p::Timestamp,
        desktop_authority: Option<std::sync::Arc<super::super::DesktopAuthority>>,
    ) -> p::Result<p::WorkerObservation> {
        let p::WorkerDispatchRequest {
            job,
            permit,
            execution,
            ..
        } = request;
        self.verify(auth)?;
        let _operation = self.operations.lock().map_err(|_| storage_error())?;
        self.validate_admission(&job, &permit, now)?;
        if let Some(execution) = &execution {
            execution.validate_for(&job, &permit, now)?;
        }

        // Duplicate dispatches must return the journaled result before asking
        // a desktop helper for a fresh liveness observation. This is what keeps
        // a terminal Unknown job from becoming a second helper attempt.
        if self
            .journal
            .existing_observation(&job.job_id, now)?
            .is_some()
        {
            let (_, observation) =
                self.journal
                    .admit_with_execution(job, permit, execution.as_ref(), now)?;
            return Ok(observation);
        }

        let admit = || {
            let desktop = match execution.as_ref() {
                Some(execution) => self.prepare_desktop_action(&job, &permit, execution, now)?,
                None => None,
            };
            let admitted = self.journal.admit_with_execution(
                job.clone(),
                permit.clone(),
                execution.as_ref(),
                now,
            )?;
            Ok((desktop, admitted))
        };
        let (desktop, (fresh, observation)) = match execution.as_ref() {
            Some(spec)
                if spec.desktop_session.is_some() || spec.backend == p::BackendKind::Computer =>
            {
                desktop_authority
                    .as_ref()
                    .ok_or_else(|| p::Error("desktop authority is unavailable".into()))?
                    .consume(&job, &permit, spec, now, admit)?
            }
            _ => admit()?,
        };
        if !fresh {
            return Ok(observation);
        }

        if let Some((request, initial_observation)) = desktop {
            self.journal.record_desktop_observation(
                &job.job_id,
                request.binding.clone(),
                initial_observation,
                now,
            )?;
            let _started = self.journal.start(&job.job_id, now)?;

            // Re-read liveness after the durable start boundary. A session can
            // be revoked between admission and the helper call.
            let live_observation =
                self.desktop_helper
                    .observe(&request.binding, now)
                    .and_then(|observation| {
                        validate_live_observation(&request.binding, &observation, now)?;
                        Ok(observation)
                    });
            let live_observation = match live_observation {
                Ok(observation) => observation,
                Err(error) => {
                    let reason = p::canonical_digest(&("desktop-session-unknown", &job.job_id))?.0;
                    self.journal.mark_unknown(&job.job_id, reason, now)?;
                    return Err(error);
                }
            };
            self.journal.record_desktop_observation(
                &job.job_id,
                request.binding.clone(),
                live_observation,
                now,
            )?;

            let intent = execution
                .as_ref()
                .map(|execution| &execution.intent)
                .ok_or_else(|| p::Error("desktop execution binding is missing".into()))?;
            let receipt = match self.desktop_helper.execute(&request, intent) {
                Ok(receipt) => receipt,
                Err(error) => {
                    let reason = p::canonical_digest(&("desktop-action-unknown", &job.job_id))?.0;
                    self.journal.mark_unknown(&job.job_id, reason, now)?;
                    return Err(error);
                }
            };
            let mut worker_receipt = worker_receipt(&request, receipt, now)?;
            if request.capability != p::DesktopCapability::Screenshot
                && worker_receipt.observed_outcome == p::WorkerOutcome::Completed
                && worker_receipt.safe_result_ref.is_none()
            {
                let binding = self.journal.result_binding(
                    &job.job_id,
                    self.installation_ref.clone(),
                    Some(worker_receipt.receipt_ref.clone()),
                )?;
                let manifest = self.result_store.seal_empty_completion(
                    binding,
                    worker_receipt.ground_truth_refs.clone(),
                    now,
                )?;
                worker_receipt.safe_result_ref = manifest.result_ref.clone();
                self.journal.record_result_manifest(manifest)?;
            } else {
                self.seal_result(WorkerResultSealRequest {
                    job: &job,
                    receipt_ref: Some(worker_receipt.receipt_ref.clone()),
                    result_ref: worker_receipt.safe_result_ref.clone(),
                    outcome: Some(worker_receipt.observed_outcome),
                    source: None,
                    evidence_refs: worker_receipt.ground_truth_refs.clone(),
                    now,
                })?;
            }
            return self.journal.record_receipt(worker_receipt, now);
        }

        // Make the Running transition durable before asking the platform to
        // create a process. If process creation succeeds and the journal write
        // were attempted afterwards, a storage failure could leave a side
        // effect without a recoverable state boundary.
        let started = self.journal.start(&job.job_id, now)?;
        if let Err(error) =
            self.isolation
                .admit_with_execution(&job, &permit, execution.as_ref(), now)
        {
            let reason = p::canonical_digest(&("worker-admission-unknown", &job.job_id))?.0;
            self.journal.mark_unknown(&job.job_id, reason, now)?;
            return Err(error);
        }
        Ok(started)
    }

    pub(super) fn prepare_desktop_action(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        execution: &p::WorkerExecutionSpec,
        now: p::Timestamp,
    ) -> p::Result<Option<(p::DesktopActionRequest, p::DesktopSessionObservation)>> {
        let Some(binding) = &execution.desktop_session else {
            if execution.backend == p::BackendKind::Computer {
                return Err(p::Error(
                    "desktop action has no independently issued grant".into(),
                ));
            }
            return Ok(None);
        };
        binding.validate_for(job, permit, now)?;
        let capability = required_capability(&execution.intent)?;
        if !binding.capabilities.contains(&capability) {
            return Err(p::Error("desktop capability is outside the grant".into()));
        }
        let observation = self.desktop_helper.observe(binding, now)?;
        validate_live_observation(binding, &observation, now)?;
        let timeout_ms = i64::try_from(execution.timeout.0)
            .map_err(|_| p::Error("desktop action timeout is too large".into()))?;
        let timeout_deadline = now
            .checked_add(timeout_ms)
            .ok_or_else(|| p::Error("desktop action deadline overflows".into()))?;
        let deadline = binding.expires_at.min(job.deadline).min(timeout_deadline);
        let request = p::DesktopActionRequest {
            schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
            job_id: job.job_id.clone(),
            permit_ref: permit.permit_ref.clone(),
            binding: binding.clone(),
            normalized_action_digest: execution.normalized_action_digest.clone(),
            capability,
            deadline,
        };
        request.validate_at(&observation, now)?;
        Ok(Some((request, observation)))
    }
}
