//! Screenshot bytes become protected material before any completion event.
use super::*;
impl WorkerActionBackend {
    /// Attach the protected material consumer before planning or dispatching a
    /// screenshot. A plain file artifact writer does not satisfy this boundary.
    pub fn with_artifact_store(mut self, artifacts: Arc<dyn crate::ArtifactStore>) -> Self {
        self.artifacts = Some(artifacts);
        self
    }
    pub(super) fn require_output_consumer(&self, plan: &ExecutionPlan) -> p::Result<()> {
        if is_screenshot(plan)
            && !self
                .artifacts
                .as_ref()
                .is_some_and(|store| store.supports_worker_output())
        {
            return Err(p::Error(
                "protected screenshot consumer is unavailable".into(),
            ));
        }
        Ok(())
    }
    pub(super) fn complete_image(
        &self,
        plan: &ExecutionPlan,
        attempt: &WorkerAttempt,
        observation: &p::WorkerObservation,
        manifest: &p::WorkerResultManifest,
        sink: &EventSink,
        started: Instant,
    ) -> p::Result<ActionResult> {
        let record = match self.verified_image(plan, attempt, observation, manifest, started) {
            Ok(record) => record,
            Err(_) => return unknown_result(plan, sink, &self.budget, &attempt.job.job_id),
        };
        let (width, height) = p::desktop_output_dimensions(&plan.intent)?;
        let receipt = p::ExternalActionReceipt {
            schema_version: p::SchemaVersion(1),
            action: plan.intent.intent_id.clone(),
            content_ref: Some(record.content_ref.clone()),
            content_digest: Some(record.digest),
            trust: p::TrustTier::Untrusted,
            effect: p::EffectStatus::Observed,
            probe_hint: Some(p::ProbeHintRef(format!("worker:{}", attempt.job.job_id))),
        };
        let mut output = BoundedOutput::new(self.budget.clone());
        let summary = format!(
            "Screenshot saved: {width} x {height} PNG, {} bytes.",
            record.bytes
        );
        output.push_with_ref(summary.as_bytes(), plan, sink, Some(record.content_ref))?;
        // Event persistence errors propagate; never append a contradictory
        // Unknown after a completion event has already been accepted.
        complete_external(plan, sink, &output, receipt)
    }
    fn verified_image(
        &self,
        plan: &ExecutionPlan,
        attempt: &WorkerAttempt,
        observation: &p::WorkerObservation,
        manifest: &p::WorkerResultManifest,
        started: Instant,
    ) -> p::Result<crate::ArtifactRecord> {
        if observation.outcome != Some(p::WorkerOutcome::Completed)
            || !is_screenshot(plan)
            || manifest.encoding != p::WorkerResultEncoding::Binary
        {
            return Err(invalid());
        }
        let descriptor = self.transport.desktop_output(&attempt.job.job_id)?;
        descriptor.validate_for(manifest)?;
        let binding = plan.desktop_session.as_ref().ok_or_else(invalid)?;
        descriptor
            .binding
            .validate_request(&p::DesktopActionRequest {
                schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
                job_id: attempt.job.job_id.clone(),
                permit_ref: attempt.permit.permit_ref.clone(),
                binding: binding.clone(),
                normalized_action_digest: p::normalized_action_digest(&plan.intent)?,
                capability: p::DesktopCapability::Screenshot,
                deadline: descriptor.binding.deadline,
            })?;
        if descriptor.binding.deadline > attempt.job.deadline
            || descriptor.binding.deadline > attempt.permit.deadline
            || manifest.produced_at < binding.issued_at
            || (descriptor.width, descriptor.height) != p::desktop_output_dimensions(&plan.intent)?
        {
            return Err(invalid());
        }
        let bytes = self.read_result(manifest, started)?;
        crate::validate_desktop_png(&bytes, descriptor.width, descriptor.height)?;
        let artifacts = self
            .artifacts
            .as_ref()
            .filter(|value| value.supports_worker_output())
            .ok_or_else(invalid)?;
        let material = p::WorkerArtifactBinding {
            schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
            manifest: manifest.clone(),
            descriptor: descriptor.clone(),
        };
        let record = artifacts.write_worker_output(&material, &bytes)?;
        let expected = p::ContentRef(format!(
            "content:{}",
            p::sha256_content_digest(material.object_id()?.as_bytes()).0
        ));
        if record.schema_version != p::SchemaVersion(1)
            || record.content_ref != expected
            || record.bytes != manifest.bytes
            || Some(&record.digest) != manifest.content_digest.as_ref()
        {
            return Err(invalid());
        }
        Ok(record)
    }
}
pub(super) fn is_screenshot(plan: &ExecutionPlan) -> bool {
    matches!(&plan.intent.parameters,p::ActionParameters::Computer(spec) if spec.operation==p::ComputerOperation::Screenshot)
}
fn invalid() -> p::Error {
    p::Error("worker screenshot output cannot be verified".into())
}
