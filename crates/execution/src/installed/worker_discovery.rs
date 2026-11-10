//! Select and verify the original admitted discovery, in journal order.
use super::*;

pub(in crate::installed) struct DiscoveryOriginal {
    pub(in crate::installed) job: p::WorkerJob,
    pub(in crate::installed) permit: p::WorkerPermit,
    pub(in crate::installed) execution: p::WorkerExecutionSpec,
    pub(in crate::installed) admitted_at: p::Timestamp,
    pub(in crate::installed) receipt: p::WorkerReceipt,
    pub(in crate::installed) manifest: p::WorkerResultManifest,
}

impl WorkerJournal {
    pub(in crate::installed) fn discovery_request(
        &self,
        job_id: &str,
    ) -> p::Result<p::WorkerDispatchRequest> {
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
        let execution = record
            .execution_binding
            .as_ref()
            .and_then(|binding| binding.discovery.as_deref())
            .ok_or_else(invalid_journal)?;
        Ok(p::WorkerDispatchRequest {
            schema_version: p::SchemaVersion(1),
            job: record.job.clone(),
            permit: record.permit.clone(),
            execution: Some(execution.clone()),
        })
    }

    pub(in crate::installed) fn discovery_original(
        &self,
        caller: &p::WorkerJob,
        server: &p::McpServerRef,
    ) -> p::Result<DiscoveryOriginal> {
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
            .values()
            .filter(|record| {
                record.job.owner_ref == caller.owner_ref
                    && record.job.authority_epoch == caller.authority_epoch
                    && record.job.workspace_scope == caller.workspace_scope
                    && record
                        .execution_binding
                        .as_ref()
                        .and_then(|binding| binding.discovery.as_deref())
                        .is_some_and(|request| {
                            matches!(&request.intent.parameters,
                        p::ActionParameters::McpDiscovery(spec) if &spec.server == server)
                        })
            })
            .max_by_key(|record| record.admitted_seq)
            .ok_or_else(invalid_journal)?;
        if record.state != p::WorkerState::Completed
            || record.job.sandbox_profile_ref != p::MANAGED_MCP_PROGRAM_PROFILE
            || record.cancel_receipt.is_some()
            || record.reason_ref.is_some()
        {
            return Err(invalid_journal());
        }
        let execution = record
            .execution_binding
            .as_ref()
            .and_then(|binding| binding.discovery.as_deref())
            .ok_or_else(invalid_journal)?;
        Ok(DiscoveryOriginal {
            job: record.job.clone(),
            permit: record.permit.clone(),
            execution: execution.clone(),
            admitted_at: record.admitted_at,
            receipt: record.receipt.clone().ok_or_else(invalid_journal)?,
            manifest: record.result_manifest.clone().ok_or_else(invalid_journal)?,
        })
    }
}
