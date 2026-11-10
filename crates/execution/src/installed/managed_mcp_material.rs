//! Original discovery material handed to the protected Core, never instructions.
use forme_protocol as p;

/// A Core-owned consumer must retain the exact verified source before the
/// discovery can complete. It cannot execute MCP or publish registry metadata.
pub trait ManagedMcpDiscoveryConsumer: Send + Sync {
    fn persist_discovery(
        &self,
        request: &p::WorkerDispatchRequest,
        observation: &p::WorkerObservation,
        manifest: &p::WorkerResultManifest,
        bytes: &[u8],
    ) -> p::Result<crate::ArtifactRecord>;
}

/// Verify original protocol objects and bytes without starting or probing a job.
/// Historical validation uses the original permit issue time, never renews it.
pub fn validate_managed_mcp_material(
    request: &p::WorkerDispatchRequest,
    observation: &p::WorkerObservation,
    manifest: &p::WorkerResultManifest,
    bytes: &[u8],
) -> p::Result<p::ManagedMcpDiscoveryCatalogV1> {
    request.validate_at(request.permit.issued_at)?;
    observation.validate()?;
    manifest.validate_for(&request.job, &request.permit)?;
    let execution = request.execution.as_ref().ok_or_else(invalid)?;
    let p::ActionParameters::McpDiscovery(spec) = &execution.intent.parameters else {
        return Err(invalid());
    };
    if request.job.sandbox_profile_ref != p::MANAGED_MCP_PROGRAM_PROFILE
        || observation.job_id != request.job.job_id
        || observation.plan_digest != request.job.plan_digest
        || observation.authority_epoch != request.job.authority_epoch
        || observation.nonce != request.job.nonce
        || observation.state != p::WorkerState::Completed
        || observation.outcome != Some(p::WorkerOutcome::Completed)
        || observation.receipt_ref.is_none()
        || observation.reason_ref.is_some()
        || manifest.binding.receipt_ref != observation.receipt_ref
        || manifest.result_ref != observation.safe_result_ref
        || manifest.evidence_refs != observation.ground_truth_refs
        || manifest.outcome != observation.outcome
        || manifest.binding.normalized_action_digest.as_ref()
            != Some(&execution.normalized_action_digest)
        || manifest.binding.output_budget_bytes != execution.output_budget_bytes
        || manifest.status != p::WorkerResultStatus::Available
        || manifest.encoding != p::WorkerResultEncoding::Utf8
        || manifest.produced_at < request.permit.issued_at
        || manifest.produced_at > observation.observed_at
        || manifest.bytes != bytes.len() as u64
        || manifest.content_digest.as_ref() != Some(&p::sha256_content_digest(bytes))
    {
        return Err(invalid());
    }
    p::ManagedMcpDiscoveryCatalogV1::from_slice(bytes, spec)
}

/// Immutable address shared by the execution consumer and its Core reader.
/// A retry with another job, permit, or result cannot replace the original.
pub fn managed_mcp_material_object(
    request: &p::WorkerDispatchRequest,
    manifest: &p::WorkerResultManifest,
) -> p::Result<String> {
    request.validate_at(request.permit.issued_at)?;
    manifest.validate_for(&request.job, &request.permit)?;
    let digest = p::canonical_digest(&(
        "forme.managed-mcp.discovery.material.v1",
        request,
        &manifest.manifest_digest,
    ))?;
    Ok(format!("managed-mcp-discovery:{}", digest.0))
}
fn invalid() -> p::Error {
    p::Error("managed MCP discovery material is unavailable or inconsistent".into())
}
