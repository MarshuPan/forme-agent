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

/// The journal keeps only the immutable execution binding, not the full
/// action parameters. This is enough to reject a changed duplicate request
/// without copying potentially large payloads into the product journal.
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
}

impl WorkerExecutionBinding {
    pub(super) fn from_spec(spec: &p::WorkerExecutionSpec) -> Self {
        Self {
            schema_version: spec.schema_version,
            backend: spec.backend,
            scope: spec.scope.clone(),
            normalized_action_digest: spec.normalized_action_digest.clone(),
            timeout: spec.timeout,
            output_budget_bytes: spec.output_budget_bytes,
            desktop_session: spec.desktop_session.clone(),
        }
    }

    pub(super) fn validate_for(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        at: p::Timestamp,
    ) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
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
        if let Some(binding) = &self.desktop_session {
            binding.validate_for(job, permit, at)?;
        }
        Ok(())
    }
}
