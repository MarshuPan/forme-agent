//! Original screenshot transfer, result sealing, cleanup and receipt ordering.
use super::*;
#[cfg(test)]
#[path = "../worker_output_host_tests.rs"]
mod tests;
use crate::installed::desktop_output_sink::{DesktopOutputAssembler, DesktopOutputSink};

impl<I: WorkerIsolation, D: DesktopHelperAdapter> WorkerExecutorHost<I, D> {
    pub(super) fn complete_desktop_output(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        execution: &p::WorkerExecutionSpec,
        request: &p::DesktopActionRequest,
        authority: &super::super::DesktopAuthority,
        started_at: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        let result = (|| {
            let binding = p::DesktopExchangeBinding::from_request(
                self.installation_ref.clone(),
                job,
                permit,
                request,
            )?;
            let transfer = p::DesktopOutputTransfer::new(
                binding,
                &execution.intent,
                execution.output_budget_bytes,
            )?;
            let mut sink = DesktopOutputAssembler::create(
                &self.result_store.desktop_staging_root(),
                transfer,
                request.clone(),
            )?;
            let receipt =
                self.desktop_helper
                    .execute_with_output(request, &execution.intent, &mut sink)?;
            if sink.receipt()? != &receipt {
                return Err(p::Error("desktop output receipt mismatch".into()));
            }
            let at = super::super::desktop_helper::now_ms()?;
            let live = self.desktop_helper.observe(&request.binding, at)?;
            validate_live_observation(&request.binding, &live, at)?;
            self.journal.record_desktop_observation(
                &job.job_id,
                request.binding.clone(),
                live,
                at,
            )?;
            let header = sink.header()?.clone();
            let bytes = sink.verified_bytes()?;
            let mut worker_receipt = worker_receipt(request, receipt, started_at)?;
            let binding = self.journal.result_binding(
                &job.job_id,
                self.installation_ref.clone(),
                Some(worker_receipt.receipt_ref.clone()),
            )?;
            let now = super::super::desktop_helper::now_ms()?;
            if now >= request.deadline {
                return Err(p::Error("desktop output sealing deadline expired".into()));
            }
            authority.with_current_consumed(&request.binding, now, || {
                let manifest = self.result_store.seal_desktop_output(
                    binding,
                    &bytes,
                    worker_receipt.ground_truth_refs.clone(),
                    now,
                )?;
                let descriptor = p::DesktopOutputDescriptor {
                    schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
                    binding: sink.transfer().binding.clone(),
                    manifest_digest: manifest.manifest_digest.clone(),
                    content_digest: header.content_digest,
                    bytes: header.bytes,
                    media_type: header.media_type,
                    width: header.width,
                    height: header.height,
                };
                descriptor.validate_for(&manifest)?;
                worker_receipt.safe_result_ref = manifest.result_ref.clone();
                self.journal.record_result_manifest(manifest)?;
                self.journal.record_desktop_output(descriptor)?;
                sink.result_sealed()?;
                let now = super::super::desktop_helper::now_ms()?;
                if now >= request.deadline {
                    return Err(p::Error(
                        "desktop output completion deadline expired".into(),
                    ));
                }
                self.journal.record_receipt(worker_receipt, now)
            })
        })();
        if result.is_err() {
            let reason = p::canonical_digest(&("desktop-output-unknown", &job.job_id))?.0;
            self.journal.mark_unknown(&job.job_id, reason, started_at)?;
        }
        result
    }
}
