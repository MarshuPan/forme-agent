use forme_protocol as p;
use serde::{Deserialize, Serialize};

pub(super) fn validate_workspace_scope(value: &p::Scope) -> p::Result<()> {
    if value.0.len() > 512
        || !value.0.starts_with("workspace:")
        || value.0.contains(['\\', '\0'])
        || value.0.chars().any(char::is_control)
        || value
            .0
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(p::Error("worker workspace scope is invalid".into()));
    }
    Ok(())
}

/// Ordinary actions keep only their immutable binding. Discovery adds a
/// bounded original request in private binding v2 so later calls can prove
/// their exact metadata source. No tool arguments or result bytes are copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkerExecutionBinding {
    pub(super) schema_version: p::SchemaVersion,
    pub(super) backend: p::BackendKind,
    pub(super) scope: p::Scope,
    pub(super) normalized_action_digest: p::SchemaDigest,
    pub(super) timeout: p::DurationMs,
    pub(super) output_budget_bytes: u64,
    #[serde(default)]
    pub(super) desktop_session: Option<p::DesktopSessionBinding>,
    #[serde(default)]
    pub(super) input_grants: Vec<p::WorkerInputGrantV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) discovery: Option<Box<p::WorkerExecutionSpec>>,
}

impl WorkerExecutionBinding {
    pub(super) fn from_spec(spec: &p::WorkerExecutionSpec) -> Self {
        let discovery = matches!(spec.intent.parameters, p::ActionParameters::McpDiscovery(_))
            .then(|| Box::new(spec.clone()));
        Self {
            schema_version: if discovery.is_some() {
                p::SchemaVersion(2)
            } else {
                spec.schema_version
            },
            backend: spec.backend,
            scope: spec.scope.clone(),
            normalized_action_digest: spec.normalized_action_digest.clone(),
            timeout: spec.timeout,
            output_budget_bytes: spec.output_budget_bytes,
            desktop_session: spec.desktop_session.clone(),
            input_grants: spec.input_grants.clone(),
            discovery,
        }
    }

    pub(super) fn matches_original(&self, other: &Self) -> bool {
        if self.discovery.is_none() && self.schema_version == p::SchemaVersion(1) {
            let mut legacy = other.clone();
            legacy.discovery = None;
            legacy.schema_version = p::SchemaVersion(1);
            *self == legacy
        } else {
            self == other
        }
    }

    pub(super) fn validate_for(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        at: p::Timestamp,
    ) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(if self.discovery.is_some() { 2 } else { 1 })
            || self.scope != job.workspace_scope
            || p::validate_product_digest(&self.normalized_action_digest).is_err()
            || self.timeout.0 == 0
            || self.timeout.0 > 600_000
            || self.timeout.0
                > job
                    .deadline
                    .saturating_sub(permit.issued_at)
                    .try_into()
                    .unwrap_or(0)
            || at >= job.deadline
            || self.output_budget_bytes == 0
            || self.output_budget_bytes > 64 * 1024 * 1024
            || job.plan_digest != permit.plan_digest
        {
            return Err(p::Error(
                "worker journal execution binding is invalid".into(),
            ));
        }
        if let Some(request) = &self.discovery {
            request.validate_for(job, permit, at)?;
            if !matches!(
                request.intent.parameters,
                p::ActionParameters::McpDiscovery(_)
            ) || job.sandbox_profile_ref != p::MANAGED_MCP_PROGRAM_PROFILE
                || Self::from_spec(request) != *self
                || serde_json::to_vec(request)
                    .map_err(|_| p::Error("discovery request is invalid".into()))?
                    .len()
                    > 16_384
            {
                return Err(p::Error("original discovery binding is invalid".into()));
            }
        }
        if let Some(binding) = &self.desktop_session {
            binding.validate_for(job, permit, at)?;
        }
        if self.input_grants.len() > p::WORKER_INPUT_MAX_GRANTS {
            return Err(p::Error("worker journal input grants are unbounded".into()));
        }
        let mut paths = std::collections::BTreeSet::new();
        for grant in &self.input_grants {
            grant.validate()?;
            if !paths.insert(grant.input_path.clone()) {
                return Err(p::Error("worker journal input paths are duplicated".into()));
            }
            if grant.job_id != job.job_id
                || grant.owner_ref != job.owner_ref
                || grant.plan_digest != job.plan_digest
                || grant.permit_digest != p::canonical_digest(permit)?
                || grant.authority_epoch != job.authority_epoch
                || grant.nonce != job.nonce
                || grant.workspace_scope != job.workspace_scope
            {
                return Err(p::Error(
                    "worker journal input grant is not job-bound".into(),
                ));
            }
        }
        Ok(())
    }
}
