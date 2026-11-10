//! Read original Host facts and protected bytes; never admit or rediscover.
use super::{worker::WorkerJournal, worker_results::WorkerResultStore};
use forme_protocol as p;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct DiscoveryProofReader {
    journal: Arc<WorkerJournal>,
    results: Arc<WorkerResultStore>,
    installation: String,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct DiscoveryOrigin {
    pub(crate) job_id: String,
    pub(crate) manifest_digest: p::SchemaDigest,
    pub(crate) catalog_digest: p::SchemaDigest,
    pub(crate) spec: p::ManagedMcpDiscoverySpec,
}

impl DiscoveryProofReader {
    pub(crate) fn verify_manifest(
        &self,
        manifest: &p::WorkerResultManifest,
    ) -> p::Result<(p::WorkerJob, p::ManagedMcpDiscoveryCatalogV1)> {
        manifest.validate()?;
        let request = self.journal.discovery_request(&manifest.binding.job_id)?;
        let p::ActionParameters::McpDiscovery(spec) = &request
            .execution
            .as_ref()
            .ok_or_else(invalid)?
            .intent
            .parameters
        else {
            return Err(invalid());
        };
        let (origin, catalog) = self.verify(&request.job, &spec.server, None)?;
        if origin.job_id != request.job.job_id
            || origin.manifest_digest != manifest.manifest_digest
            || self.journal.result_manifest(&request.job.job_id)?.as_ref() != Some(manifest)
        {
            return Err(invalid());
        }
        Ok((request.job, catalog))
    }

    pub(crate) fn new(
        journal: Arc<WorkerJournal>,
        results: Arc<WorkerResultStore>,
        installation: String,
    ) -> Self {
        Self {
            journal,
            results,
            installation,
        }
    }

    /// Caller holds configuration exclusion. A newer pending or failed
    /// discovery shadows the older result rather than silently reusing it.
    pub(crate) fn verify(
        &self,
        caller: &p::WorkerJob,
        server: &p::McpServerRef,
        expected: Option<&DiscoveryOrigin>,
    ) -> p::Result<(DiscoveryOrigin, p::ManagedMcpDiscoveryCatalogV1)> {
        let original = self.journal.discovery_original(caller, server)?;
        let request = &original.execution;
        request.validate_for(&original.job, &original.permit, original.admitted_at)?;
        let p::ActionParameters::McpDiscovery(spec) = &request.intent.parameters else {
            return Err(invalid());
        };
        let receipt = &original.receipt;
        let manifest = &original.manifest;
        receipt.validate_for(&original.job)?;
        manifest.validate_for(&original.job, &original.permit)?;
        if receipt.observed_outcome != p::WorkerOutcome::Completed
            || manifest.outcome != Some(p::WorkerOutcome::Completed)
            || manifest.status != p::WorkerResultStatus::Available
            || manifest.encoding != p::WorkerResultEncoding::Utf8
            || manifest.truncated
            || manifest.bytes > spec.output_budget_bytes
            || manifest.binding.installation_ref != self.installation
            || manifest.binding.normalized_action_digest.as_ref()
                != Some(&request.normalized_action_digest)
            || manifest.binding.output_budget_bytes != request.output_budget_bytes
            || manifest.binding.receipt_ref.as_ref() != Some(&receipt.receipt_ref)
            || manifest.result_ref != receipt.safe_result_ref
            || manifest.result_ref.is_none()
            || manifest.evidence_refs != receipt.ground_truth_refs
            || receipt.finished_at.is_none()
        {
            return Err(invalid());
        }
        let bytes = self
            .results
            .read_original(manifest, spec.output_budget_bytes)?;
        let catalog = p::ManagedMcpDiscoveryCatalogV1::from_slice(&bytes, spec)?;
        let origin = DiscoveryOrigin {
            job_id: original.job.job_id,
            manifest_digest: manifest.manifest_digest.clone(),
            catalog_digest: catalog.catalog_digest.clone(),
            spec: spec.clone(),
        };
        if expected.is_some_and(|expected| expected != &origin) {
            return Err(invalid());
        }
        Ok((origin, catalog))
    }
}
fn invalid() -> p::Error {
    p::Error("managed MCP original discovery is unavailable or changed".into())
}
