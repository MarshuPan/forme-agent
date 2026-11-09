//! Pending registration and grant acquisition never dispatch a worker.
use super::*;

#[derive(Clone)]
pub(super) struct DesktopAttempt {
    request: p::DesktopGrantRequest,
    binding: Option<p::DesktopSessionBinding>,
}

impl WorkerActionBackend {
    pub fn register_desktop_pending(
        &self,
        plan: &ExecutionPlan,
        run: p::RunId,
        approval: p::ApprovalId,
        policy_version: p::Version,
        tool_schema_version: p::Version,
        expires_at: p::Timestamp,
    ) -> p::Result<p::DesktopPendingAction> {
        validate_plan_for(plan, BackendKind::Computer)?;
        if self.kind != BackendKind::Computer || plan.desktop_session.is_some() {
            return Err(unavailable());
        }
        let pending = p::DesktopPendingAction {
            schema_version: p::DESKTOP_AUTHORITY_SCHEMA_VERSION,
            pending_ref: p::desktop_pending_ref(&approval, &plan.digest)?,
            owner_ref: self.owner.clone(),
            authority_epoch: self.authority_epoch,
            run_ref: run,
            approval_ref: approval,
            action_ref: plan.intent.intent_id.clone(),
            plan_digest: plan.digest.clone(),
            normalized_action_digest: plan.normalized_action_digest.clone(),
            affected_resources: plan.affected_resources.clone(),
            capability: p::desktop_intent_capability(&plan.intent)?,
            policy_version,
            tool_schema_version,
            expires_at,
        };
        pending.validate()?;
        let registered = self.transport.register_desktop_pending(pending.clone())?;
        if registered != pending {
            return Err(unavailable());
        }
        Ok(registered)
    }

    /// A missing consent does not reserve a job, nonce, or grant operation.
    pub fn require_desktop_consent(&self, plan: &ExecutionPlan) -> p::Result<()> {
        let approval = plan.approval_ref.as_ref().ok_or_else(unavailable)?;
        let pending_ref = p::desktop_pending_ref(approval, &plan.digest)?;
        let consent = self.transport.desktop_pending_consent(&pending_ref)?;
        if consent.schema_version != p::DESKTOP_AUTHORITY_SCHEMA_VERSION
            || consent.pending_ref != pending_ref
            || consent.consent_ref != p::desktop_consent_ref(&pending_ref)?
            || consent.revision == 0
            || consent.status != p::DesktopConsentStatus::Committed
            || consent.expires_at <= now_ms()
        {
            return Err(unavailable());
        }
        Ok(())
    }

    /// Called after the final Harness recheck and before approval consumption.
    /// Re-entry can only query the original grant operation.
    pub fn prepare_desktop(&self, plan: ExecutionPlan) -> p::Result<ExecutionPlan> {
        validate_plan_for(&plan, BackendKind::Computer)?;
        self.require_desktop_consent(&plan)?;
        if self.kind != BackendKind::Computer || plan.desktop_session.is_some() {
            return Err(unavailable());
        }
        let pending_ref = p::desktop_pending_ref(
            plan.approval_ref.as_ref().ok_or_else(unavailable)?,
            &plan.digest,
        )?;
        let (request, issue) = {
            let mut attempts = self.attempts.lock().map_err(|_| unavailable())?;
            let attempt = self.reserve(&mut attempts, &plan, now_ms())?;
            if attempt.dispatch_attempted {
                return Err(unavailable());
            }
            if let Some(desktop) = &attempt.desktop {
                (desktop.request.clone(), false)
            } else {
                let request = p::DesktopGrantRequest {
                    schema_version: p::DESKTOP_AUTHORITY_SCHEMA_VERSION,
                    operation_id: format!("desktop-issue:{}", plan.digest.0),
                    consent_ref: p::desktop_consent_ref(&pending_ref)?,
                    job: attempt.job.clone(),
                    permit: attempt.permit.clone(),
                    execution: execution_spec(&plan),
                };
                request.validate_at(now_ms())?;
                attempt.desktop = Some(DesktopAttempt {
                    request: request.clone(),
                    binding: None,
                });
                (request, true)
            }
        };
        let view = if issue {
            self.transport
                .acquire_desktop_grant(request.clone())
                .or_else(|_| self.transport.desktop_grant(&request.operation_id))?
        } else {
            self.transport.desktop_grant(&request.operation_id)?
        };
        view.binding
            .validate_for(&request.job, &request.permit, now_ms())?;
        let digest = request.digest()?;
        if view.schema_version != p::DESKTOP_AUTHORITY_SCHEMA_VERSION
            || view.operation_id != request.operation_id
            || view.revision == 0
            || view.status != p::DesktopGrantStatus::Issued
            || view.binding.grant_ref != format!("desktop-grant:{}", &digest.0[7..])
            || view.binding.capabilities
                != [p::desktop_intent_capability(&plan.intent)?]
                    .into_iter()
                    .collect()
        {
            return Err(unavailable());
        }
        let mut attempts = self.attempts.lock().map_err(|_| unavailable())?;
        let attempt = attempts
            .get_mut(&plan.intent.intent_id)
            .ok_or_else(unavailable)?;
        let desktop = attempt.desktop.as_mut().ok_or_else(unavailable)?;
        if desktop.request != request
            || attempt.dispatch_attempted
            || desktop
                .binding
                .as_ref()
                .is_some_and(|old| old != &view.binding)
        {
            return Err(unavailable());
        }
        desktop.binding = Some(view.binding.clone());
        plan.with_desktop_session(view.binding)
    }

    fn reserve<'a>(
        &self,
        attempts: &'a mut BTreeMap<p::ActionId, WorkerAttempt>,
        plan: &ExecutionPlan,
        now: p::Timestamp,
    ) -> p::Result<&'a mut WorkerAttempt> {
        use std::collections::btree_map::Entry;
        let attempt = match attempts.entry(plan.intent.intent_id.clone()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(build_attempt(
                plan,
                self.owner.clone(),
                self.authority_epoch,
                self.sandbox_profile_ref.clone(),
                now,
            )?),
        };
        if attempt.job.plan_digest != plan.digest {
            return Err(p::Error(
                "action already has a worker job for another plan".into(),
            ));
        }
        Ok(attempt)
    }

    pub(super) fn claim_dispatch(
        &self,
        plan: &ExecutionPlan,
        now: p::Timestamp,
    ) -> p::Result<(WorkerAttempt, bool)> {
        let mut attempts = self.attempts.lock().map_err(|_| unavailable())?;
        if self.kind == BackendKind::Computer && !attempts.contains_key(&plan.intent.intent_id) {
            return Err(unavailable());
        }
        let attempt = self.reserve(&mut attempts, plan, now)?;
        if self.kind == BackendKind::Computer {
            let original = attempt
                .desktop
                .as_ref()
                .and_then(|desktop| desktop.binding.as_ref())
                .ok_or_else(unavailable)?;
            if plan.desktop_session.as_ref() != Some(original) {
                return Err(unavailable());
            }
            if !attempt.dispatch_attempted {
                original.validate_for(&attempt.job, &attempt.permit, now)?;
            }
        }
        let fresh = !attempt.dispatch_attempted;
        attempt.dispatch_attempted = true;
        Ok((attempt.clone(), fresh))
    }
}

fn unavailable() -> p::Error {
    p::Error(
        "desktop consent or original grant is unavailable; execution remains unadmitted".into(),
    )
}
