//! A call consumes the original completed discovery under live configuration.
use super::*;
use crate::installed::managed_mcp_proof::{DiscoveryOrigin, DiscoveryProofReader};
use admission::DiscoveryAdmission;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(crate) struct ManagedCall {
    pub(crate) tool: String,
    pub(crate) arguments: serde_json::Value,
    pub(crate) output_budget: u64,
    schema_digest: p::SchemaDigest,
    timeout: p::DurationMs,
    proof: DiscoveryProofReader,
    origin: DiscoveryOrigin,
}

impl InstalledManagedMcpPrograms {
    pub(crate) fn with_call<T>(
        &self,
        request: &p::WorkerDispatchRequest,
        proof: DiscoveryProofReader,
        now: p::Timestamp,
        entered: Instant,
        action: impl FnOnce(DiscoveryAdmission) -> p::Result<T>,
    ) -> p::Result<T> {
        let execution = request.execution.as_ref().ok_or_else(invalid)?;
        execution.validate_for(&request.job, &request.permit, now)?;
        let p::ActionParameters::Mcp {
            server,
            tool,
            arguments,
            schema_digest,
            transport,
            stdio,
            timeout,
        } = &execution.intent.parameters
        else {
            return Err(invalid());
        };
        if request.job.sandbox_profile_ref != p::MANAGED_MCP_PROGRAM_PROFILE
            || request.job.authority_epoch != self.epoch
            || *transport != p::McpTransport::Stdio
            || stdio.schema_version != p::SchemaVersion(1)
            || !stdio.args.is_empty()
            || !stdio.env.is_empty()
            || timeout.0 == 0
            || timeout.0 > execution.timeout.0
            || !arguments.is_object()
        {
            return Err(invalid());
        }
        p::validate_product_id(&stdio.command)?;
        let remaining = u64::try_from(request.job.deadline.checked_sub(now).ok_or_else(invalid)?)
            .map_err(|_| invalid())?
            .min(timeout.0);
        let deadline = entered
            .checked_add(Duration::from_millis(remaining))
            .ok_or_else(invalid)?;
        let cancel = crate::CancelToken::default();
        let mut config = self.configuration_until(deadline, &cancel)?;
        let registry = self.registry_for(&config.snapshot()?)?;
        let (origin, _) = proof.verify(&request.job, server, None)?;
        if stdio.command != origin.spec.program_ref {
            return Err(invalid());
        }
        let call = ManagedCall {
            tool: tool.0.clone(),
            arguments: arguments.clone(),
            schema_digest: schema_digest.clone().ok_or_else(invalid)?,
            timeout: *timeout,
            output_budget: execution.output_budget_bytes,
            proof,
            origin,
        };
        self.check_discovery(&mut config, &request.job, &call.origin.spec)?;
        call.recheck(&mut config, &request.job)?;
        let lease = registry.acquire(
            &p::OwnerPrincipalRef(request.job.owner_ref.0.clone()),
            self.epoch,
            &call.origin.spec.program_ref,
            &call.origin.spec.registration_digest,
            &request.job.workspace_scope,
        )?;
        admission::live(deadline, &cancel)?;
        let result = action(DiscoveryAdmission {
            spec: call.origin.spec.clone(),
            lease: Arc::new(lease),
            deadline,
            job: request.job.clone(),
            registry,
            call: Some(Arc::new(call)),
        });
        drop(config);
        result
    }
}

impl ManagedCall {
    pub(super) fn recheck(
        &self,
        config: &mut ProductConfigStore,
        job: &p::WorkerJob,
    ) -> p::Result<()> {
        let snapshot = config.snapshot_settings_v3()?;
        let settings = snapshot
            .managed_capabilities
            .mcp_servers
            .iter()
            .find(|settings| settings.server_ref == self.origin.spec.server.0)
            .ok_or_else(invalid)?;
        self.origin
            .spec
            .validate_settings(settings, &job.workspace_scope)?;
        if !settings.allowlist.tools.contains(&self.tool)
            || self.timeout.0 > settings.timeout.0
            || self.output_budget == 0
            || self.output_budget > settings.output_budget
            || self.output_budget > 1_048_576
        {
            return Err(invalid());
        }
        let (_, catalog) = self
            .proof
            .verify(job, &self.origin.spec.server, Some(&self.origin))?;
        let tool = catalog
            .tools
            .iter()
            .find(|tool| tool.name == self.tool)
            .ok_or_else(invalid)?;
        if tool.schema_digest != self.schema_digest {
            return Err(invalid());
        }
        // Match the existing bounded argument admission. This is not a full
        // JSON Schema implementation; schema identity is checked separately.
        if let Some(required) = tool.input_schema.get("required") {
            for key in required.as_array().ok_or_else(invalid)? {
                if self
                    .arguments
                    .get(key.as_str().ok_or_else(invalid)?)
                    .is_none()
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}
