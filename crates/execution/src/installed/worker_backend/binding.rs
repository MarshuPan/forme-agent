//! Immutable worker job, observation and result bindings.
use super::*;
pub(super) fn build_attempt(
    plan: &ExecutionPlan,
    owner: p::VerifiedPrincipal,
    authority_epoch: p::AuthorityEpoch,
    sandbox_profile_ref: String,
    issued_at: p::Timestamp,
) -> p::Result<WorkerAttempt> {
    let suffix = plan.digest.0.clone();
    let deadline = issued_at
        .checked_add(
            i64::try_from(plan.timeout.0)
                .map_err(|_| p::Error("worker timeout is too large".into()))?,
        )
        .ok_or_else(|| p::Error("worker deadline overflows".into()))?;
    let job_id = format!("worker-job:{suffix}");
    let permit_ref = format!("worker-permit:{suffix}");
    let nonce = p::Nonce(format!("worker-nonce:{suffix}"));
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: job_id.clone(),
        owner_ref: owner.clone(),
        action_ref: plan.intent.intent_id.clone(),
        affected_resources: plan.affected_resources.clone(),
        execution_plan_ref: format!("execution-plan:{suffix}"),
        plan_digest: plan.digest.clone(),
        authority_epoch,
        permit_ref: permit_ref.clone(),
        sandbox_profile_ref: sandbox_profile_ref.clone(),
        workspace_scope: plan.scope.clone(),
        deadline,
        nonce: nonce.clone(),
        cancel_ref: format!("worker-cancel:{suffix}"),
    };
    let permit = p::WorkerPermit {
        schema_version: p::SchemaVersion(1),
        permit_ref,
        job_id,
        owner_ref: owner,
        action_ref: plan.intent.intent_id.clone(),
        affected_resources: plan.affected_resources.clone(),
        plan_digest: plan.digest.clone(),
        authority_epoch,
        workspace_scope: plan.scope.clone(),
        sandbox_profile_ref,
        deadline,
        nonce,
        issued_at,
    };
    permit.validate_for(&job, issued_at)?;
    Ok(WorkerAttempt {
        job,
        permit,
        dispatch_attempted: false,
        desktop: None,
    })
}

pub(super) fn execution_spec(plan: &ExecutionPlan) -> p::WorkerExecutionSpec {
    p::WorkerExecutionSpec {
        schema_version: p::SchemaVersion(1),
        plan_digest: plan.digest.clone(),
        intent: plan.intent.clone(),
        backend: plan.backend,
        scope: plan.scope.clone(),
        affected_resources: plan.affected_resources.clone(),
        normalized_action_digest: plan.normalized_action_digest.clone(),
        timeout: plan.timeout,
        output_budget_bytes: plan.budget.max_bytes,
        desktop_session: plan.desktop_session.clone(),
        input_grants: plan.input_grants.clone(),
    }
}

pub(super) fn validate_observation(
    observation: &p::WorkerObservation,
    job: &p::WorkerJob,
) -> p::Result<()> {
    observation.validate()?;
    if observation.job_id != job.job_id
        || observation.plan_digest != job.plan_digest
        || observation.authority_epoch != job.authority_epoch
        || observation.nonce != job.nonce
    {
        return Err(p::Error(
            "worker observation is not bound to the job".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_manifest(
    manifest: &p::WorkerResultManifest,
    attempt: &WorkerAttempt,
    plan: &ExecutionPlan,
    observation: &p::WorkerObservation,
) -> p::Result<()> {
    manifest.validate()?;
    manifest
        .binding
        .validate_for(&attempt.job, &attempt.permit)?;
    if manifest.binding.normalized_action_digest.as_ref() != Some(&plan.normalized_action_digest)
        || manifest.binding.output_budget_bytes != plan.budget.max_bytes
        || manifest.binding.permit_digest != p::canonical_digest(&attempt.permit)?
        || manifest.binding.receipt_ref != observation.receipt_ref
        || observation.receipt_ref.is_none()
        || manifest.result_ref != observation.safe_result_ref
        || manifest.outcome != observation.outcome
    {
        return Err(p::Error(
            "worker result is not bound to the terminal job".into(),
        ));
    }
    Ok(())
}

pub(super) fn external_receipt(
    plan: &ExecutionPlan,
    manifest: &p::WorkerResultManifest,
    effect: p::EffectStatus,
) -> p::ExternalActionReceipt {
    p::ExternalActionReceipt {
        schema_version: p::SchemaVersion(1),
        action: plan.intent.intent_id.clone(),
        content_ref: manifest.result_ref.clone().map(p::ContentRef),
        content_digest: manifest.content_digest.clone(),
        trust: p::TrustTier::Untrusted,
        effect,
        probe_hint: Some(p::ProbeHintRef(format!(
            "worker:{}",
            manifest.binding.job_id
        ))),
    }
}

pub(super) fn unknown_result(
    plan: &ExecutionPlan,
    sink: &EventSink,
    budget: &OutputBudget,
    job_id: &str,
) -> p::Result<ActionResult> {
    emit_managed_call(plan, sink, Some("server_error"))?;
    outcome_unknown(
        plan,
        sink,
        &BoundedOutput::new(budget.clone()),
        p::ProbeHintRef(format!("worker:{job_id}")),
    )
}

pub(super) fn is_terminal(state: p::WorkerState) -> bool {
    matches!(
        state,
        p::WorkerState::Completed
            | p::WorkerState::Failed
            | p::WorkerState::Cancelled
            | p::WorkerState::Unknown
    )
}

pub(super) fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

pub(super) fn emit_managed_call(
    plan: &ExecutionPlan,
    sink: &EventSink,
    error: Option<&str>,
) -> p::Result<()> {
    if let p::ActionParameters::Mcp { server, tool, .. } = &plan.intent.parameters {
        sink.emit(p::EventPayload::McpCallEvent(p::McpCallEventPayload {
            server: server.clone(),
            tool: tool.clone(),
            timeout: false,
            error_class: error.map(|class| p::McpErrorClass(class.into())),
        }))?;
    }
    Ok(())
}
