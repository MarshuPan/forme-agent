use std::sync::Arc;

use forme_protocol as p;

use super::{storage_error, HostAuth, WorkerExecutorHost, WorkerIsolation};
use crate::installed::desktop_helper::DesktopHelperAdapter;
use crate::installed::worker_input::{WorkerInputMaterial, WorkerInputResolver};

impl<I: WorkerIsolation, D: DesktopHelperAdapter> WorkerExecutorHost<I, D> {
    /// Install the executor-local resolver used for approved ContentRef and
    /// opaque Secret slot grants. The default remains rejecting.
    pub fn with_input_resolver(mut self, resolver: Arc<dyn WorkerInputResolver>) -> Self {
        self.input_resolver = resolver;
        self
    }

    pub(super) fn resolve_inputs(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        execution: &p::WorkerExecutionSpec,
        now: p::Timestamp,
    ) -> p::Result<Vec<WorkerInputMaterial>> {
        let mut resolved = Vec::with_capacity(execution.input_grants.len());
        let mut bytes = 0_u64;
        for grant in &execution.input_grants {
            if grant.installation_ref != self.installation_ref {
                return Err(p::Error(
                    "worker input grant belongs to another installation".into(),
                ));
            }
            let material =
                self.input_resolver
                    .resolve(grant, job, permit, &execution.intent, now)?;
            bytes = bytes
                .checked_add(material.bytes().len() as u64)
                .filter(|size| *size <= p::WORKER_INPUT_MAX_TOTAL_BYTES)
                .ok_or_else(|| p::Error("worker input material exceeds its byte budget".into()))?;
            resolved.push(material);
        }
        Ok(resolved)
    }

    pub(super) fn read_input(
        &self,
        auth: &HostAuth,
        request: p::WorkerInputReadRequest,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerInputChunk> {
        self.verify(auth)?;
        request.validate()?;
        let _operation = self.operations.lock().map_err(|_| storage_error())?;
        let (job, permit, stored) = self
            .journal
            .input_grant(&request.grant.job_id, &request.grant.grant_ref)?;
        if stored != request.grant
            || stored.installation_ref != self.installation_ref
            || job.authority_epoch != self.authority_epoch
        {
            return Err(p::Error(
                "worker input grant does not match its journal binding".into(),
            ));
        }
        let observation = self.journal.observation(&job.job_id, now)?;
        if !matches!(
            observation.state,
            p::WorkerState::Pending | p::WorkerState::Running
        ) {
            return Err(p::Error("worker input grant is no longer readable".into()));
        }
        let material = self
            .input_resolver
            .resolve_content(&stored, &job, &permit, now)?;
        let bytes = material.bytes();
        let total = stored
            .content_bytes
            .ok_or_else(|| p::Error("content input size is missing".into()))?;
        if request.offset > total || request.offset > bytes.len() as u64 {
            return Err(p::Error("worker input offset is invalid".into()));
        }
        let end = request
            .offset
            .saturating_add(request.max_bytes)
            .min(total)
            .min(bytes.len() as u64);
        let slice = &bytes[request.offset as usize..end as usize];
        let chunk = p::WorkerInputChunk {
            schema_version: p::WORKER_INPUT_SCHEMA_VERSION,
            grant_ref: stored.grant_ref,
            offset: request.offset,
            next_offset: end,
            bytes: slice.to_vec(),
            finished: end == total,
            content_digest: stored
                .content_digest
                .ok_or_else(|| p::Error("content input digest is missing".into()))?,
        };
        chunk.validate()?;
        Ok(chunk)
    }
}
