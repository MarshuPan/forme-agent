//! Observe the original controller, seal results, then revoke its sandbox lease.
use super::*;
impl WorkerIsolation for ManagedMcpIsolation {
    fn with_quiescent_resources(&self, action: &mut dyn FnMut() -> p::Result<()>) -> p::Result<()> {
        super::super::worker_isolation::with_empty_registry(&self.children, &mut || {
            if self.blocked || !retained().lock().map_err(|_| invalid())?.is_empty() {
                return Err(invalid());
            }
            self.journal.with_quiescent_leases(action)
        })
    }
    fn admit(&self, _: &p::WorkerJob, _: &p::WorkerPermit) -> p::Result<()> {
        Err(invalid())
    }
    fn cancel(&self, job: &p::WorkerJob) -> p::Result<p::WorkerCancelReceipt> {
        self.cancel_at(job, recovery_timestamp())
    }
    fn cancel_at(
        &self,
        job: &p::WorkerJob,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerCancelReceipt> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            {
                let mut workers = self.children.lock().map_err(|_| invalid())?;
                let worker = workers.get_mut(&job.job_id).ok_or_else(invalid)?;
                worker.matches(job)?;
                worker.controller.cancel();
                if worker.cleanup_confirmed()? {
                    let digest = p::canonical_digest(&("forme-managed-mcp-cancel-v1", job))?.0;
                    return Ok(p::WorkerCancelReceipt {
                        schema_version: p::SchemaVersion(1),
                        job_id: job.job_id.clone(),
                        plan_digest: job.plan_digest.clone(),
                        nonce: job.nonce.clone(),
                        outcome: if worker.controller.call_attempted() {
                            p::WorkerOutcome::Unknown
                        } else {
                            p::WorkerOutcome::Cancelled
                        },
                        observed_at: now,
                        receipt_ref: Some(format!("worker-cancel-{digest}")),
                        reason_ref: worker
                            .controller
                            .call_attempted()
                            .then(|| "managed-mcp-call-outcome-unknown".into()),
                    });
                }
            }
            if Instant::now() >= deadline {
                return Err(invalid());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn observe(
        &self,
        job: &p::WorkerJob,
        now: p::Timestamp,
    ) -> p::Result<Option<p::WorkerReceipt>> {
        let mut workers = self.children.lock().map_err(|_| invalid())?;
        let worker = workers.get_mut(&job.job_id).ok_or_else(invalid)?;
        worker.matches(job)?;
        if let Some(receipt) = &worker.receipt {
            return Ok(Some(receipt.clone()));
        }
        if !worker.cleanup_confirmed()? {
            if Instant::now() >= worker.admission.deadline + Duration::from_secs(2) {
                return Err(invalid());
            }
            return Ok(None);
        }
        let digest = p::canonical_digest(&("forme-managed-mcp-result-v1", job))?.0;
        let (body, outcome, finished_at) = match worker.controller.result() {
            Ok((bytes, outcome, at)) => (Some(bytes), outcome, at),
            Err(_) if worker.controller.call_attempted() => (None, p::WorkerOutcome::Unknown, now),
            Err(_) if worker.admission.call.is_some() => {
                (Some(&[][..]), p::WorkerOutcome::Failed, now)
            }
            Err(_) => (None, p::WorkerOutcome::Failed, now),
        };
        if let Some(bytes) = body {
            if bytes.len() as u64 > worker.admission.output_budget() {
                return Err(invalid());
            }
            let results = worker.dir.join("results");
            if !worker.result_written {
                fs::create_dir(&results).map_err(|_| invalid())?;
                set_private_directory(&results)?;
                write_result(&results.join("result.bin"), bytes)?;
                worker.result_written = true;
            }
        }
        let receipt = p::WorkerReceipt {
            schema_version: p::SchemaVersion(1),
            receipt_ref: format!("worker-receipt-{digest}"),
            job_id: job.job_id.clone(),
            plan_digest: job.plan_digest.clone(),
            authority_epoch: job.authority_epoch,
            nonce: job.nonce.clone(),
            observed_outcome: outcome,
            safe_result_ref: worker
                .result_written
                .then(|| format!("worker-result-{digest}")),
            ground_truth_refs: vec![format!("worker-mcp-tree-io-confirmed-{digest}")],
            started_at: worker.started_at,
            finished_at: Some(finished_at),
        };
        receipt.validate_for(job)?;
        worker.receipt = Some(receipt.clone());
        Ok(Some(receipt))
    }
    fn result_path(&self, job: &p::WorkerJob) -> p::Result<Option<PathBuf>> {
        let mut workers = self.children.lock().map_err(|_| invalid())?;
        let worker = workers.get_mut(&job.job_id).ok_or_else(invalid)?;
        worker.matches(job)?;
        if !worker.cleanup_confirmed()? {
            return Err(invalid());
        }
        Ok(worker
            .result_written
            .then(|| worker.dir.join("results/result.bin")))
    }
    fn result_sealed(&self, job: &p::WorkerJob, now: p::Timestamp) -> p::Result<()> {
        let mut workers = self.children.lock().map_err(|_| invalid())?;
        let worker = workers.get_mut(&job.job_id).ok_or_else(invalid)?;
        worker.matches(job)?;
        if !worker.cleanup_confirmed()? {
            return Err(invalid());
        }
        results::acknowledge_handoff(&worker.dir, job)?;
        let profile = match &worker.native {
            Some(native) => native.profile(),
            None => worker.setup.profile().ok_or_else(invalid)?,
        };
        recovery::cleanup_lease(
            &self.journal,
            &worker.lease_id,
            &worker.dir,
            profile,
            (true, 0),
            now,
        )?;
        workers.remove(&job.job_id);
        Ok(())
    }
}

fn write_result(path: &Path, bytes: &[u8]) -> p::Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| invalid())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| invalid())
}
