use crate::{BackendKind, ExecutionPlanner};
use forme_protocol as p;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputBudget {
    pub schema_version: p::SchemaVersion,
    pub max_bytes: u64,
    pub truncate: bool,
}

impl OutputBudget {
    pub fn truncate_at(max_bytes: u64) -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            max_bytes,
            truncate: true,
        }
    }

    pub fn strict(max_bytes: u64) -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            max_bytes,
            truncate: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    pub schema_version: p::SchemaVersion,
    pub digest: p::PlanDigest,
    pub normalized_action_digest: p::SchemaDigest,
    pub affected_resources: Vec<p::ResourceRef>,
    pub intent: p::ActionIntent,
    pub backend: BackendKind,
    pub scope: p::Scope,
    pub approval_ref: Option<p::ApprovalId>,
    pub verify_ref: Option<p::VerifyRef>,
    pub rollback_boundary: p::RollbackBoundary,
    pub budget: OutputBudget,
    pub timeout: p::DurationMs,
    /// A session grant is bound to this plan after planning. It is deliberately
    /// outside `PlanDigest`: the grant carries the digest and is checked again
    /// at WorkerJob admission, while the plan digest remains stable for
    /// approval and replay.
    pub desktop_session: Option<p::DesktopSessionBinding>,
}

impl ExecutionPlan {
    pub fn validate_digest(&self) -> p::Result<()> {
        validate_discovery_bounds(&self.intent, &self.budget, self.timeout)?;
        let normalized_action_digest = p::normalized_action_digest(&self.intent)?;
        if normalized_action_digest != self.normalized_action_digest {
            return Err(p::Error(
                "execution plan action digest does not match its intent".into(),
            ));
        }
        let affected_resources = p::affected_resources_for(&self.intent);
        if affected_resources != self.affected_resources {
            return Err(p::Error(
                "execution plan affected resources do not match its intent".into(),
            ));
        }
        let expected = digest_plan(self)?;
        if expected != self.digest {
            return Err(p::Error(
                "execution plan digest no longer matches its contents".into(),
            ));
        }
        if let Some(binding) = &self.desktop_session {
            binding.validate()?;
            if binding.action_ref != self.intent.intent_id || binding.plan_digest != self.digest {
                return Err(p::Error(
                    "execution plan desktop session is not bound to the plan".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> p::Result<()> {
        self.normalized_action_digest = p::normalized_action_digest(&self.intent)?;
        self.affected_resources = p::affected_resources_for(&self.intent);
        self.digest = digest_plan(self)?;
        Ok(())
    }

    /// Approval identity is intentionally excluded from `PlanDigest`: the
    /// request is created from the digest first, then the returned approval id
    /// is attached and checked independently by the harness.
    pub fn with_approval(mut self, approval: p::ApprovalId) -> Self {
        self.approval_ref = Some(approval);
        self
    }

    /// Attach a Host-issued session grant after the base plan digest exists.
    /// The grant is not permission by itself; the final worker admission
    /// revalidates its owner, action, plan, epoch, nonce and deadline.
    pub fn with_desktop_session(mut self, binding: p::DesktopSessionBinding) -> p::Result<Self> {
        binding.validate()?;
        if binding.action_ref != self.intent.intent_id || binding.plan_digest != self.digest {
            return Err(p::Error(
                "desktop session grant does not match the execution plan".into(),
            ));
        }
        self.desktop_session = Some(binding);
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionStatus {
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub schema_version: p::SchemaVersion,
    pub path: String,
    pub operation: p::FileOperation,
    pub previous_bytes: u64,
    pub next_bytes: u64,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRollback {
    pub schema_version: p::SchemaVersion,
    pub path: String,
    pub previous: Option<Vec<u8>>,
    pub remove_if_previously_absent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionResult {
    pub schema_version: p::SchemaVersion,
    pub result_ref: p::ActionResultRef,
    pub status: ActionStatus,
    pub output_ref: p::OutputRef,
    pub output: String,
    pub truncated: bool,
    pub evidence: p::CapabilityEvidence,
    pub diff: Option<FileDiff>,
    pub rollback: Option<FileRollback>,
    pub external_receipt: Option<p::ExternalActionReceipt>,
}

pub struct ResultNormalizer;

impl ResultNormalizer {
    pub(crate) fn completed(
        plan: &ExecutionPlan,
        output: String,
        truncated: bool,
        diff: Option<FileDiff>,
        rollback: Option<FileRollback>,
    ) -> ActionResult {
        Self::result(
            plan,
            ActionStatus::Completed,
            output,
            truncated,
            "success",
            ResultAttachments {
                diff,
                rollback,
                external_receipt: None,
            },
        )
    }

    pub(crate) fn cancelled(plan: &ExecutionPlan, output: String, truncated: bool) -> ActionResult {
        Self::result(
            plan,
            ActionStatus::Cancelled,
            output,
            truncated,
            "cancelled",
            ResultAttachments::default(),
        )
    }

    fn result(
        plan: &ExecutionPlan,
        status: ActionStatus,
        output: String,
        truncated: bool,
        outcome: &str,
        attachments: ResultAttachments,
    ) -> ActionResult {
        ActionResult {
            schema_version: p::SchemaVersion(1),
            result_ref: p::ActionResultRef(format!("action-result:{}", plan.intent.intent_id.0)),
            status,
            output_ref: p::OutputRef(format!("action-output:{}", plan.intent.intent_id.0)),
            output,
            truncated,
            evidence: p::CapabilityEvidence {
                schema_version: p::SchemaVersion(1),
                capability: plan.intent.capability_ref.clone(),
                outcome: p::CapabilityOutcome(outcome.into()),
                reliability: p::Reliability("observed".into()),
            },
            diff: attachments.diff,
            rollback: attachments.rollback,
            external_receipt: attachments.external_receipt,
        }
    }

    pub(crate) fn completed_external(
        plan: &ExecutionPlan,
        output: String,
        truncated: bool,
        receipt: p::ExternalActionReceipt,
    ) -> ActionResult {
        Self::result(
            plan,
            ActionStatus::Completed,
            output,
            truncated,
            "success",
            ResultAttachments {
                external_receipt: Some(receipt),
                ..ResultAttachments::default()
            },
        )
    }

    pub(crate) fn outcome_unknown(
        plan: &ExecutionPlan,
        output: String,
        truncated: bool,
        receipt: p::ExternalActionReceipt,
    ) -> ActionResult {
        Self::result(
            plan,
            ActionStatus::Unknown,
            output,
            truncated,
            "unknown",
            ResultAttachments {
                external_receipt: Some(receipt),
                ..ResultAttachments::default()
            },
        )
    }
}

#[derive(Default)]
struct ResultAttachments {
    diff: Option<FileDiff>,
    rollback: Option<FileRollback>,
    external_receipt: Option<p::ExternalActionReceipt>,
}

#[derive(Debug, Clone)]
pub struct DefaultExecutionPlanner {
    budget: OutputBudget,
    timeout: p::DurationMs,
}

impl DefaultExecutionPlanner {
    pub fn new(budget: OutputBudget, timeout: p::DurationMs) -> Self {
        Self { budget, timeout }
    }
}

impl ExecutionPlanner for DefaultExecutionPlanner {
    fn plan(&self, intent: &p::ActionIntent) -> p::Result<ExecutionPlan> {
        plan_with(intent, self.budget.clone(), self.timeout)
    }
}

pub(crate) fn plan_with(
    intent: &p::ActionIntent,
    budget: OutputBudget,
    timeout: p::DurationMs,
) -> p::Result<ExecutionPlan> {
    if budget.max_bytes == 0 || timeout.0 == 0 {
        return Err(p::Error(
            "execution budget and timeout must be non-zero".into(),
        ));
    }
    validate_backend_parameters(intent)?;
    validate_discovery_bounds(intent, &budget, timeout)?;
    let rollback_boundary = rollback_boundary(intent);
    let mut plan = ExecutionPlan {
        schema_version: p::SchemaVersion(1),
        digest: p::PlanDigest(String::new()),
        normalized_action_digest: p::SchemaDigest(String::new()),
        affected_resources: Vec::new(),
        intent: intent.clone(),
        backend: intent.backend_hint,
        scope: intent.scope.clone(),
        approval_ref: None,
        verify_ref: None,
        rollback_boundary,
        budget,
        timeout,
        desktop_session: None,
    };
    plan.refresh_digest()?;
    Ok(plan)
}

pub(crate) fn validate_plan_for(plan: &ExecutionPlan, backend: BackendKind) -> p::Result<()> {
    plan.validate_digest()?;
    if plan.backend != backend
        || plan.intent.backend_hint != backend
        || plan.scope != plan.intent.scope
    {
        return Err(p::Error(
            "execution plan is routed to the wrong backend".into(),
        ));
    }
    validate_backend_parameters(&plan.intent)
}

fn validate_discovery_bounds(
    intent: &p::ActionIntent,
    budget: &OutputBudget,
    timeout: p::DurationMs,
) -> p::Result<()> {
    if let p::ActionParameters::McpDiscovery(spec) = &intent.parameters {
        spec.validate_intent(intent)?;
        if budget.truncate
            || spec.output_budget_bytes > budget.max_bytes
            || spec.timeout.0 > timeout.0
        {
            return Err(p::Error(
                "discovery requires complete output within the execution bounds".into(),
            ));
        }
    }
    Ok(())
}

fn validate_backend_parameters(intent: &p::ActionIntent) -> p::Result<()> {
    let matches = matches!(
        (&intent.backend_hint, &intent.parameters),
        (p::BackendKind::Shell, p::ActionParameters::Shell { .. })
            | (p::BackendKind::File, p::ActionParameters::File { .. })
            | (p::BackendKind::Mcp, p::ActionParameters::Mcp { .. })
            | (p::BackendKind::Mcp, p::ActionParameters::McpDiscovery(_))
            | (
                p::BackendKind::Notification,
                p::ActionParameters::Notification { .. }
            )
            | (p::BackendKind::Browser, p::ActionParameters::Browser(_))
            | (p::BackendKind::Computer, p::ActionParameters::Computer(_))
            | (p::BackendKind::Pty, p::ActionParameters::Pty(_))
            | (p::BackendKind::AppApi, p::ActionParameters::AppApi(_))
            | (p::BackendKind::Remote, p::ActionParameters::Remote(_))
    );
    if !matches {
        return Err(p::Error("backend and action parameters disagree".into()));
    }
    Ok(())
}

fn rollback_boundary(intent: &p::ActionIntent) -> p::RollbackBoundary {
    match &intent.parameters {
        p::ActionParameters::File {
            operation: p::FileOperation::Write | p::FileOperation::Delete,
            ..
        } => p::RollbackBoundary("file-content-snapshot".into()),
        p::ActionParameters::Notification { .. } => {
            p::RollbackBoundary("notification-not-retractable".into())
        }
        p::ActionParameters::Browser(spec) => match spec.operation {
            p::BrowserOperation::Navigate
            | p::BrowserOperation::ReadText { .. }
            | p::BrowserOperation::Screenshot { .. } => p::RollbackBoundary("none".into()),
            p::BrowserOperation::Click { .. } | p::BrowserOperation::Type { .. } => {
                p::RollbackBoundary("browser-effect-not-retractable".into())
            }
        },
        p::ActionParameters::Computer(spec) => match spec.operation {
            p::ComputerOperation::Screenshot => p::RollbackBoundary("none".into()),
            _ => p::RollbackBoundary("computer-effect-not-retractable".into()),
        },
        p::ActionParameters::Pty(_) => p::RollbackBoundary("process-effect-not-retractable".into()),
        p::ActionParameters::AppApi(spec) => match spec.operation {
            p::AppApiOperation::Read => p::RollbackBoundary("read-only-no-rollback-needed".into()),
            p::AppApiOperation::Mutation { .. } => {
                p::RollbackBoundary("api-mutation-not-retractable".into())
            }
        },
        p::ActionParameters::Remote(spec) => spec.placement.operation.rollback_boundary.clone(),
        _ => p::RollbackBoundary("none".into()),
    }
}

fn digest_plan(plan: &ExecutionPlan) -> p::Result<p::PlanDigest> {
    let material = serde_json::json!({
        "schema_version": plan.schema_version,
        "normalized_action_digest": plan.normalized_action_digest,
        "affected_resources": plan.affected_resources,
        "backend": plan.backend,
        "scope": plan.scope,
        "verify_ref": plan.verify_ref,
        "rollback_boundary": plan.rollback_boundary,
        "budget": {
            "schema_version": plan.budget.schema_version,
            "max_bytes": plan.budget.max_bytes,
            "truncate": plan.budget.truncate,
        },
        "timeout": plan.timeout,
    });
    Ok(p::PlanDigest(p::canonical_digest(&material)?.0))
}
