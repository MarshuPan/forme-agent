//! Immutable encrypted originals; neither paths nor request bodies are sidecars.
use super::*;
use serde::de::DeserializeOwned;

const MAX_BYTES: usize = 64 * 1024;

fn object(run: &p::RunId, stage: &str) -> String {
    format!("owner-mcp-discovery-v1:{stage}:{}", run.0)
}
fn reference(run: &p::RunId, stage: &str) -> p::ContentRef {
    p::ContentRef(format!(
        "content:{}",
        p::sha256_content_digest(object(run, stage).as_bytes()).0
    ))
}

pub(super) fn read<T: DeserializeOwned>(
    harness: &ReactiveHarness,
    run: &p::RunId,
    stage: &str,
) -> p::Result<Option<T>> {
    read_from(
        &harness.store,
        harness.configured_owner.as_ref().ok_or_else(invalid)?,
        &p::Scope(harness.config.workspace.0.clone()),
        run,
        stage,
    )
}

pub(super) fn write<T: Serialize + DeserializeOwned + PartialEq>(
    harness: &ReactiveHarness,
    run: &p::RunId,
    stage: &str,
    scope: &p::Scope,
    value: &T,
) -> p::Result<()> {
    write_to(
        &harness.store,
        harness.configured_owner.as_ref().ok_or_else(invalid)?,
        scope,
        run,
        stage,
        value,
    )
}

pub(super) fn read_from<T: DeserializeOwned>(
    store: &SqliteEventStore,
    owner: &p::VerifiedPrincipal,
    scope: &p::Scope,
    run: &p::RunId,
    stage: &str,
) -> p::Result<Option<T>> {
    let Some(record) = store.protected_content_for_current_owner(&reference(run, stage))? else {
        return Ok(None);
    };
    if record.owner_digest != p::sha256_content_digest(owner.0.as_bytes())
        || record.scope.0 != scope.0
        || record.subject_digest != p::sha256_content_digest(record.scope.0.as_bytes())
        || record.envelope.ciphertext.len() > MAX_BYTES + 16
    {
        return Err(invalid());
    }
    let protection = store.content_protection()?.ok_or_else(invalid)?;
    let bytes = protection
        .content_service()
        .decrypt(&record.envelope.binding(), &record.envelope)?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| invalid())
}

pub(super) fn write_to<T: Serialize + DeserializeOwned + PartialEq>(
    store: &SqliteEventStore,
    owner: &p::VerifiedPrincipal,
    scope: &p::Scope,
    run: &p::RunId,
    stage: &str,
    value: &T,
) -> p::Result<()> {
    if let Some(prior) = read_from::<T>(store, owner, scope, run, stage)? {
        return if prior == *value {
            Ok(())
        } else {
            Err(invalid())
        };
    }
    let protection = store.content_protection()?.ok_or_else(invalid)?;
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid());
    }
    let (_, sealed) = protection.seal_record(
        &bytes,
        scope,
        &object(run, stage),
        &p::sha256_content_digest(scope.0.as_bytes()),
    )?;
    if sealed.content_ref != reference(run, stage) {
        return Err(invalid());
    }
    protection
        .lifecycle_service(store.clone())
        .put_protected_content(owner, sealed)?;
    if read_from::<T>(store, owner, scope, run, stage)?.as_ref() != Some(value) {
        return Err(invalid());
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlanOriginal {
    pub(super) execution: p::WorkerExecutionSpec,
    budget_version: p::SchemaVersion,
    truncate: bool,
    verify_ref: Option<p::VerifyRef>,
    rollback: p::RollbackBoundary,
}
impl PlanOriginal {
    pub(super) fn capture(plan: &ExecutionPlan) -> p::Result<Self> {
        plan.validate_digest()?;
        if plan.approval_ref.is_some() || plan.desktop_session.is_some() || plan.budget.truncate {
            return Err(invalid());
        }
        Ok(Self {
            execution: p::WorkerExecutionSpec {
                schema_version: plan.schema_version,
                plan_digest: plan.digest.clone(),
                intent: plan.intent.clone(),
                backend: plan.backend,
                scope: plan.scope.clone(),
                affected_resources: plan.affected_resources.clone(),
                normalized_action_digest: plan.normalized_action_digest.clone(),
                timeout: plan.timeout,
                output_budget_bytes: plan.budget.max_bytes,
                desktop_session: None,
            },
            budget_version: plan.budget.schema_version,
            truncate: plan.budget.truncate,
            verify_ref: plan.verify_ref.clone(),
            rollback: plan.rollback_boundary.clone(),
        })
    }
    pub(super) fn restore(&self) -> p::Result<ExecutionPlan> {
        let e = &self.execution;
        let plan = ExecutionPlan {
            schema_version: e.schema_version,
            digest: e.plan_digest.clone(),
            normalized_action_digest: e.normalized_action_digest.clone(),
            affected_resources: e.affected_resources.clone(),
            intent: e.intent.clone(),
            backend: e.backend,
            scope: e.scope.clone(),
            approval_ref: None,
            verify_ref: self.verify_ref.clone(),
            rollback_boundary: self.rollback.clone(),
            budget: forme_execution::OutputBudget {
                schema_version: self.budget_version,
                max_bytes: e.output_budget_bytes,
                truncate: self.truncate,
            },
            timeout: e.timeout,
            desktop_session: e.desktop_session.clone(),
        };
        if Self::capture(&plan)? != *self {
            return Err(invalid());
        }
        Ok(plan)
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PendingDiscovery {
    pub(super) schema_version: p::SchemaVersion,
    pub(super) original: OwnerRequest,
    pub(super) plan: PlanOriginal,
    pub(super) approval: p::PendingApproval,
    pub(super) session_binding: p::SessionBoundPayload,
    pub(super) verification_passes: u16,
}

impl ReactiveHarness {
    pub(crate) fn persist_owner_discovery_pending(
        &self,
        run: &p::RunId,
        record: &RunRecord,
        intent: &p::ActionIntent,
        plan: &ExecutionPlan,
        approval: &ApprovalRequest,
    ) -> p::Result<()> {
        let Some(original) = self.bound_discovery(run, record, intent)? else {
            return Ok(());
        };
        if approval.expires_at > original.envelope.timebox.expires_at {
            return Err(invalid());
        }
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let session_binding = events
            .iter()
            .find_map(|e| match &e.payload {
                p::EventPayload::SessionBound(v) => Some(v.clone()),
                _ => None,
            })
            .ok_or_else(invalid)?;
        let verification_passes = record
            .m3_binding
            .as_ref()
            .and_then(|b| b.model_scaffold.as_ref())
            .map_or(1, |v| v.scaffold.verification_passes);
        if verification_passes == 0 {
            return Err(invalid());
        }
        let pending = PendingDiscovery {
            schema_version: p::SchemaVersion(1),
            original,
            plan: PlanOriginal::capture(plan)?,
            approval: pending_approval(run.clone(), approval.clone()),
            session_binding,
            verification_passes,
        };
        write(self, run, "pending", &intent.scope, &pending)
    }

    pub(crate) fn discovery_verification_passes(
        &self,
        run: &p::RunId,
        record: &RunRecord,
    ) -> p::Result<Option<u16>> {
        if !record.owner_discovery_bound {
            return Ok(None);
        }
        let pending: PendingDiscovery = read(self, run, "pending")?.ok_or_else(invalid)?;
        if pending.schema_version != p::SchemaVersion(1)
            || pending.original.request != record.request
            || pending.verification_passes == 0
        {
            return Err(invalid());
        }
        Ok(Some(pending.verification_passes))
    }
}
