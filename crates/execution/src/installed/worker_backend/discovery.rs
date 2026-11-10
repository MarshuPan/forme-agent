//! A completed discovery is retained before its brain completion is published.
use super::super::managed_mcp_material::{
    managed_mcp_material_object, validate_managed_mcp_material, ManagedMcpDiscoveryConsumer,
};
use super::*;
impl WorkerActionBackend {
    pub fn with_discovery_consumer(
        mut self,
        consumer: Arc<dyn ManagedMcpDiscoveryConsumer>,
    ) -> Self {
        self.discovery_consumer = Some(consumer);
        self
    }
    pub(super) fn complete_discovery(
        &self,
        plan: &ExecutionPlan,
        attempt: &WorkerAttempt,
        observation: &p::WorkerObservation,
        manifest: &p::WorkerResultManifest,
        bytes: &[u8],
        sink: &EventSink,
    ) -> p::Result<ActionResult> {
        let protected = || {
            let request = p::WorkerDispatchRequest {
                schema_version: p::SchemaVersion(1),
                job: attempt.job.clone(),
                permit: attempt.permit.clone(),
                execution: Some(execution_spec(plan)),
            };
            let catalog = validate_managed_mcp_material(&request, observation, manifest, bytes)?;
            let record = self
                .discovery_consumer
                .as_ref()
                .ok_or_else(invalid)?
                .persist_discovery(&request, observation, manifest, bytes)?;
            let object = managed_mcp_material_object(&request, manifest)?;
            let expected = p::ContentRef(format!(
                "content:{}",
                p::sha256_content_digest(object.as_bytes()).0
            ));
            if record.schema_version != p::SchemaVersion(1)
                || record.content_ref != expected
                || Some(&record.digest) != manifest.content_digest.as_ref()
                || record.bytes != manifest.bytes
            {
                return Err(invalid());
            }
            Ok((catalog, record))
        };
        let (catalog, record) = match protected() {
            Ok(value) => value,
            Err(_) => return unknown_result(plan, sink, &self.budget, &attempt.job.job_id),
        };
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
            "MCP discovery retained: {} tools, {} resources.",
            catalog.tools.len(),
            catalog.resources.len()
        );
        output.push_with_ref(summary.as_bytes(), plan, sink, Some(record.content_ref))?;
        complete_external(plan, sink, &output, receipt)
    }
}
fn invalid() -> p::Error {
    p::Error("protected MCP discovery consumer is unavailable or inconsistent".into())
}
