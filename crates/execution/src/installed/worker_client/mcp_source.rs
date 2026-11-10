use super::*;
use crate::installed::ManagedMcpSourceVerifier;

impl ManagedMcpSourceVerifier for WorkerBrokerClient {
    fn verify_source(
        &self,
        original: &p::WorkerResultManifest,
    ) -> p::Result<p::ProductMcpServerConfig> {
        original.validate()?;
        if original.binding.installation_ref
            != super::super::worker_results::installation_ref(&p::canonical_digest(
                &self.credential.as_str(),
            )?)?
        {
            return Err(p::Error(
                "MCP source belongs to another installation".into(),
            ));
        }
        let body = self.json(
            "POST",
            &format!("/v1/agent/workers/{}/mcp-source", original.binding.job_id),
            Some(original),
            Operation::McpSource,
        )?;
        let settings: p::ProductMcpServerConfig = decode(&body, Operation::McpSource)?;
        settings.validate()?;
        if settings.scope != original.binding.workspace_scope {
            return Err(Operation::McpSource.transport_error());
        }
        Ok(settings)
    }
}
