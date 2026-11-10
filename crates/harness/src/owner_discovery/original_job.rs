//! Immutable dispatch originals are written before the first worker message.
use super::*;
use persistence::{read_from, write_to};

pub(crate) fn recorder(
    store: SqliteEventStore,
    owner: p::VerifiedPrincipal,
    scope: p::Scope,
    worker: p::SchemaDigest,
    epoch: p::AuthorityEpoch,
) -> impl Fn(&p::WorkerDispatchRequest) -> p::Result<()> + Send + Sync {
    move |request| {
        let suffix = request
            .job
            .action_ref
            .0
            .strip_prefix("action:owner-mcp:")
            .ok_or_else(invalid)?;
        let run = p::RunId(format!("run:owner-mcp:{suffix}"));
        store.require_current_execution(&run)?;
        let pending = originals(&store, &owner, &scope, &run, request)?;
        if pending.original.worker != worker
            || request.job.authority_epoch != epoch
            || request.permit.issued_at < pending.original.envelope.timebox.starts_at
            || request.permit.issued_at >= pending.original.envelope.timebox.expires_at
        {
            return Err(invalid());
        }
        let events = store.read_run(run.clone()).collect::<p::Result<Vec<_>>>()?;
        validate_authorization(&pending, request, &events)?;
        if events.iter().any(|e| {
            matches!(
                e.kind,
                p::EventKind::ActionStarted
                    | p::EventKind::ActionCompleted
                    | p::EventKind::ActionOutcomeUnknown
            )
        }) {
            return Err(invalid());
        }
        write_to(&store, &owner, &scope, &run, "dispatch", request)
    }
}

fn originals(
    store: &SqliteEventStore,
    owner: &p::VerifiedPrincipal,
    scope: &p::Scope,
    run: &p::RunId,
    request: &p::WorkerDispatchRequest,
) -> p::Result<PendingDiscovery> {
    request.validate_at(request.permit.issued_at)?;
    let original: OwnerRequest =
        read_from(store, owner, scope, run, "request")?.ok_or_else(invalid)?;
    let pending: PendingDiscovery =
        read_from(store, owner, scope, run, "pending")?.ok_or_else(invalid)?;
    let execution = request.execution.as_ref().ok_or_else(invalid)?;
    if original.schema_version != p::SchemaVersion(1)
        || pending.schema_version != p::SchemaVersion(1)
        || pending.original != original
        || original.owner != *owner
        || original.run != *run
        || request.job.owner_ref != *owner
        || request.job.workspace_scope != *scope
        || request.job.action_ref != original.intent.intent_id
        || original.intent != execution.intent
        || original.request.source != p::Source::OwnerControl
        || original.envelope.approval_rule != p::ApprovalRule::Ask
        || pending.plan.execution != *execution
        || pending.plan.restore()?.digest != pending.approval.plan_digest
        || pending.approval.run != *run
        || pending.approval.session.0 != original.request.session.0
        || pending.approval.expires_at <= request.permit.issued_at
        || pending.verification_passes == 0
        || request.job.sandbox_profile_ref != p::MANAGED_MCP_PROGRAM_PROFILE
        || !matches!(
            execution.intent.parameters,
            p::ActionParameters::McpDiscovery(_)
        )
    {
        return Err(invalid());
    }
    Ok(pending)
}

pub(super) fn validate_authorization(
    pending: &PendingDiscovery,
    request: &p::WorkerDispatchRequest,
    events: &[p::Event],
) -> p::Result<()> {
    let original = &pending.original;
    let approval = &pending.approval;
    let mut accepted = false;
    let mut bound = false;
    let mut asked = false;
    let mut granted = false;
    let mut planned = false;
    for (index, event) in events.iter().enumerate() {
        if event.run_id != original.run
            || event.stream_seq != index as u64 + 1
            || event.schema_version != p::SchemaVersion(1)
            || event.kind != event.payload.kind()
        {
            return Err(invalid());
        }
        if matches!(
            event.kind,
            p::EventKind::RunAccepted
                | p::EventKind::SessionBound
                | p::EventKind::ApprovalRequested
                | p::EventKind::ApprovalResolved
                | p::EventKind::ActionPlanned
        ) && event.provenance != internal_provenance(p::Source::OwnerControl)
        {
            return Err(invalid());
        }
        match &event.payload {
            p::EventPayload::RunAccepted(v) => {
                if accepted
                    || v.source != original.request.source
                    || v.input_ref.0 != original.request.input.0
                    || v.session_ref.0 != original.request.session.0
                    || v.idempotency_key != original.request.idempotency_key
                {
                    return Err(invalid());
                }
                accepted = true;
            }
            p::EventPayload::SessionBound(v) => {
                if !accepted
                    || bound
                    || *v != pending.session_binding
                    || v.composition_manifest.as_ref() != Some(&original.composition)
                {
                    return Err(invalid());
                }
                bound = true;
            }
            p::EventPayload::ApprovalRequested(v) => {
                if !bound
                    || asked
                    || v.approval_id != approval.approval_id
                    || v.plan_digest.as_ref() != Some(&approval.plan_digest)
                    || v.normalized_action_digest.as_ref()
                        != Some(&approval.normalized_action_digest)
                    || v.affected_resources != approval.affected_resources
                    || v.policy_version != Some(approval.policy_version)
                    || v.tool_schema_version != Some(approval.tool_schema_version)
                    || v.scope != approval.scope
                    || v.expires_at != approval.expires_at
                    || v.risk != p::Risk::High
                {
                    return Err(invalid());
                }
                asked = true;
            }
            p::EventPayload::ApprovalResolved(v) => {
                if !asked
                    || granted
                    || v.approval_id != approval.approval_id
                    || v.outcome != p::ApprovalOutcome::Granted
                    || v.grant_ref.is_none()
                {
                    return Err(invalid());
                }
                granted = true;
            }
            p::EventPayload::ActionPlanned(v) => {
                if !granted
                    || planned
                    || v.intent_id != original.intent.intent_id
                    || v.plan_digest != request.job.plan_digest
                    || v.backend != p::BackendKind::Mcp
                    || v.scope != original.intent.scope
                    || v.approval_ref.as_ref() != Some(&approval.approval_id)
                    || v.source != p::Source::OwnerControl
                    || v.expected_effect != p::ExpectedEffect::Outward
                    || v.remote_placement.is_some()
                {
                    return Err(invalid());
                }
                planned = true;
            }
            p::EventPayload::ActionDenied(_)
            | p::EventPayload::ActionFailed(_)
            | p::EventPayload::ActionCancelled(_)
            | p::EventPayload::RunFailed(_)
            | p::EventPayload::RunAborted(_)
            | p::EventPayload::ModelCallStarted(_) => {
                return Err(invalid());
            }
            _ => (),
        }
    }
    if !planned {
        return Err(invalid());
    }
    Ok(())
}

/// This proof only permits accepting a historical Unknown before completion;
/// it cannot authorize a new dispatch or a new approval.
pub(crate) fn matches_retained(
    store: &SqliteEventStore,
    owner: &p::VerifiedPrincipal,
    run: &p::RunId,
    request: &p::WorkerDispatchRequest,
) -> p::Result<bool> {
    let scope = &request.job.workspace_scope;
    let Some(saved) = read_from::<p::WorkerDispatchRequest>(store, owner, scope, run, "dispatch")?
    else {
        return Ok(false);
    };
    if saved != *request {
        return Err(invalid());
    }
    let pending = originals(store, owner, scope, run, request)?;
    validate_authorization(
        &pending,
        request,
        &store.read_run(run.clone()).collect::<p::Result<Vec<_>>>()?,
    )?;
    Ok(true)
}
