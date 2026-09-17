//! Gateway-neutral approvals bound to an immutable execution plan (prd/04).
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use forme_policy::ArgMatcher;
use forme_protocol as p;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub schema_version: p::SchemaVersion,
    pub approval_id: p::ApprovalId,
    pub session: p::SessionId,
    pub action_summary: String,
    pub risk_level: p::Risk,
    pub scope: p::Scope,
    pub requested_permissions: Vec<p::PermissionRef>,
    pub affected_resources: Vec<p::ResourceRef>,
    pub rollback_boundary: p::RollbackBoundary,
    pub expires_at: p::Timestamp,
    pub choices: Vec<p::ApprovalChoice>,
    pub plan_digest: p::PlanDigest,
    pub policy_version: p::Version,
    pub tool_schema_version: p::Version,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalTicket(pub p::ApprovalId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalScope {
    All,
    Session(p::SessionId),
}

pub type ApprovalOutcome = p::ApprovalOutcome;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantScope {
    OneShot,
    Session,
    ParamPattern(ArgMatcher),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalGrant {
    pub schema_version: p::SchemaVersion,
    pub approval_id: p::ApprovalId,
    pub outcome: p::ApprovalOutcome,
    pub granted_scope: GrantScope,
    pub approver: p::VerifiedPrincipal,
    pub bound_plan_digest: p::PlanDigest,
    pub policy_version: p::Version,
    pub tool_schema_version: p::Version,
    pub nonce: p::Nonce,
    pub use_by: p::Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalAuthorization {
    pub approval_id: p::ApprovalId,
    pub session: p::SessionId,
    pub scope: p::Scope,
    pub plan_digest: p::PlanDigest,
    pub policy_version: p::Version,
    pub tool_schema_version: p::Version,
    pub now: p::Timestamp,
    pub intent: p::ActionIntent,
}

pub trait ApprovalBroker {
    fn request(&self, req: ApprovalRequest) -> p::Result<ApprovalTicket>;
    fn resolve(&self, ticket: ApprovalTicket, grant: ApprovalGrant) -> p::Result<()>;
    fn pending(&self, scope: ApprovalScope) -> Vec<ApprovalRequest>;
}

type Clock = Arc<dyn Fn() -> p::Timestamp + Send + Sync>;

pub struct InMemoryApprovalBroker {
    clock: Clock,
    state: Mutex<BrokerState>,
}

#[derive(Debug, Default)]
struct BrokerState {
    pending: BTreeMap<p::ApprovalId, ApprovalRequest>,
    active: BTreeMap<p::ApprovalId, ActiveGrant>,
    known_ids: BTreeSet<p::ApprovalId>,
    nonces: BTreeSet<p::Nonce>,
    events: Vec<p::EventPayload>,
}

#[derive(Debug, Clone)]
struct ActiveGrant {
    request: ApprovalRequest,
    grant: ApprovalGrant,
    consumed: bool,
}

impl Default for InMemoryApprovalBroker {
    fn default() -> Self {
        Self::with_clock(system_timestamp)
    }
}

impl InMemoryApprovalBroker {
    pub fn with_clock<F>(clock: F) -> Self
    where
        F: Fn() -> p::Timestamp + Send + Sync + 'static,
    {
        Self {
            clock: Arc::new(clock),
            state: Mutex::new(BrokerState::default()),
        }
    }

    pub fn authorize(&self, authorization: &ApprovalAuthorization) -> p::Result<()> {
        let clock_now = (self.clock)();
        let now = clock_now.max(authorization.now);
        let mut state = self.lock_state()?;
        let active = state
            .active
            .get_mut(&authorization.approval_id)
            .ok_or_else(|| p::Error("approval grant is not active".into()))?;

        if active.grant.outcome != p::ApprovalOutcome::Granted {
            return Err(p::Error(
                "approval outcome does not authorize an action".into(),
            ));
        }
        if now >= active.grant.use_by || now >= active.request.expires_at {
            return Err(p::Error("approval grant has expired".into()));
        }
        if authorization.session != active.request.session
            || authorization.scope != active.request.scope
            || authorization.intent.scope != active.request.scope
        {
            return Err(p::Error("approval scope does not match the action".into()));
        }
        if authorization.plan_digest != active.grant.bound_plan_digest
            || authorization.plan_digest != active.request.plan_digest
        {
            return Err(p::Error("execution plan changed after approval".into()));
        }
        if authorization.policy_version != active.grant.policy_version
            || authorization.policy_version != active.request.policy_version
            || authorization.tool_schema_version != active.grant.tool_schema_version
            || authorization.tool_schema_version != active.request.tool_schema_version
        {
            return Err(p::Error("approval environment version changed".into()));
        }
        if !authorization
            .intent
            .requested_permissions
            .iter()
            .all(|permission| active.request.requested_permissions.contains(permission))
        {
            return Err(p::Error("action requests an unapproved permission".into()));
        }

        match &active.grant.granted_scope {
            GrantScope::OneShot if active.consumed => {
                return Err(p::Error("one-shot approval was already consumed".into()));
            }
            GrantScope::OneShot => active.consumed = true,
            GrantScope::Session => {}
            GrantScope::ParamPattern(pattern) if pattern.matches(&authorization.intent) => {}
            GrantScope::ParamPattern(_) => {
                return Err(p::Error(
                    "action parameters exceed the approved pattern".into(),
                ));
            }
        }

        Ok(())
    }

    pub fn take_events(&self) -> Vec<p::EventPayload> {
        self.lock_state()
            .map(|mut state| std::mem::take(&mut state.events))
            .unwrap_or_default()
    }

    fn lock_state(&self) -> p::Result<MutexGuard<'_, BrokerState>> {
        self.state
            .lock()
            .map_err(|_| p::Error("approval broker state is unavailable".into()))
    }
}

impl ApprovalBroker for InMemoryApprovalBroker {
    fn request(&self, request: ApprovalRequest) -> p::Result<ApprovalTicket> {
        let now = (self.clock)();
        if request.expires_at <= now {
            return Err(p::Error("approval request is already expired".into()));
        }
        if request.action_summary.trim().is_empty()
            || request.choices.is_empty()
            || request.plan_digest.0.trim().is_empty()
        {
            return Err(p::Error("approval request is incomplete".into()));
        }

        let mut state = self.lock_state()?;
        if !state.known_ids.insert(request.approval_id.clone()) {
            return Err(p::Error("approval id has already been used".into()));
        }

        let ticket = ApprovalTicket(request.approval_id.clone());
        state.events.push(requested_event(&request));
        state.pending.insert(request.approval_id.clone(), request);
        Ok(ticket)
    }

    fn resolve(&self, ticket: ApprovalTicket, grant: ApprovalGrant) -> p::Result<()> {
        let now = (self.clock)();
        let mut state = self.lock_state()?;
        let request = state
            .pending
            .get(&ticket.0)
            .cloned()
            .ok_or_else(|| p::Error("approval ticket is not pending".into()))?;

        validate_grant(&request, &ticket, &grant, now, &state.nonces)?;

        state.pending.remove(&ticket.0);
        state.nonces.insert(grant.nonce.clone());
        let grant_ref = if grant.outcome == p::ApprovalOutcome::Granted {
            let grant_ref = p::ApprovalGrantRef(format!("approval-grant:{}", grant.approval_id.0));
            state.active.insert(
                grant.approval_id.clone(),
                ActiveGrant {
                    request,
                    grant: grant.clone(),
                    consumed: false,
                },
            );
            Some(grant_ref)
        } else {
            None
        };
        state.events.push(p::EventPayload::ApprovalResolved(
            p::ApprovalResolvedPayload {
                approval_id: grant.approval_id,
                outcome: grant.outcome,
                grant_ref,
            },
        ));
        Ok(())
    }

    fn pending(&self, scope: ApprovalScope) -> Vec<ApprovalRequest> {
        let now = (self.clock)();
        self.lock_state()
            .map(|state| {
                state
                    .pending
                    .values()
                    .filter(|request| request.expires_at > now)
                    .filter(|request| match &scope {
                        ApprovalScope::All => true,
                        ApprovalScope::Session(session) => &request.session == session,
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn validate_grant(
    request: &ApprovalRequest,
    ticket: &ApprovalTicket,
    grant: &ApprovalGrant,
    now: p::Timestamp,
    used_nonces: &BTreeSet<p::Nonce>,
) -> p::Result<()> {
    if request.expires_at <= now {
        return Err(p::Error(
            "approval request expired before resolution".into(),
        ));
    }
    if ticket.0 != request.approval_id || grant.approval_id != request.approval_id {
        return Err(p::Error(
            "approval grant targets a different request".into(),
        ));
    }
    if grant.approver.0.trim().is_empty() {
        return Err(p::Error("approval requires a verified principal".into()));
    }
    if grant.bound_plan_digest != request.plan_digest
        || grant.policy_version != request.policy_version
        || grant.tool_schema_version != request.tool_schema_version
    {
        return Err(p::Error(
            "approval binding does not match the request".into(),
        ));
    }
    if grant.nonce.0.trim().is_empty() || used_nonces.contains(&grant.nonce) {
        return Err(p::Error("approval nonce is invalid or already used".into()));
    }
    if grant.use_by <= now || grant.use_by > request.expires_at {
        return Err(p::Error("approval grant expiry exceeds its request".into()));
    }
    Ok(())
}

fn requested_event(request: &ApprovalRequest) -> p::EventPayload {
    p::EventPayload::ApprovalRequested(p::ApprovalRequestedPayload {
        approval_id: request.approval_id.clone(),
        action_summary: p::ActionSummary(request.action_summary.clone()),
        risk: request.risk_level,
        scope: request.scope.clone(),
        rollback_boundary: request.rollback_boundary.clone(),
        expires_at: request.expires_at,
        choices: request.choices.clone(),
        requested_permissions: request.requested_permissions.clone(),
        affected_resources: request.affected_resources.clone(),
    })
}

fn system_timestamp() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
