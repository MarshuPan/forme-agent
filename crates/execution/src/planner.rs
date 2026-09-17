use forme_protocol as p;
use sha2::{Digest, Sha256};

use crate::{BackendKind, ExecutionPlanner};

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
    pub intent: p::ActionIntent,
    pub backend: BackendKind,
    pub scope: p::Scope,
    pub approval_ref: Option<p::ApprovalId>,
    pub verify_ref: Option<p::VerifyRef>,
    pub rollback_boundary: p::RollbackBoundary,
    pub budget: OutputBudget,
    pub timeout: p::DurationMs,
}

impl ExecutionPlan {
    pub fn validate_digest(&self) -> p::Result<()> {
        let expected = digest_plan(self)?;
        if expected != self.digest {
            return Err(p::Error(
                "execution plan digest no longer matches its contents".into(),
            ));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> p::Result<()> {
        self.digest = digest_plan(self)?;
        Ok(())
    }

    pub fn with_approval(mut self, approval: p::ApprovalId) -> Self {
        self.approval_ref = Some(approval);
        self
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
    let rollback_boundary = rollback_boundary(intent);
    let mut plan = ExecutionPlan {
        schema_version: p::SchemaVersion(1),
        digest: p::PlanDigest(String::new()),
        intent: intent.clone(),
        backend: intent.backend_hint,
        scope: intent.scope.clone(),
        approval_ref: None,
        verify_ref: None,
        rollback_boundary,
        budget,
        timeout,
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

fn validate_backend_parameters(intent: &p::ActionIntent) -> p::Result<()> {
    let matches = matches!(
        (&intent.backend_hint, &intent.parameters),
        (p::BackendKind::Shell, p::ActionParameters::Shell { .. })
            | (p::BackendKind::File, p::ActionParameters::File { .. })
            | (p::BackendKind::Mcp, p::ActionParameters::Mcp { .. })
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
        "intent": plan.intent,
        "backend": plan.backend,
        "scope": plan.scope,
        "rollback_boundary": plan.rollback_boundary,
        "budget": {
            "schema_version": plan.budget.schema_version,
            "max_bytes": plan.budget.max_bytes,
            "truncate": plan.budget.truncate,
        },
        "timeout": plan.timeout,
    });
    let bytes = serde_json::to_vec(&material)
        .map_err(|error| p::Error(format!("failed to normalize execution plan: {error}")))?;
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(7 + digest.len() * 2);
    encoded.push_str("sha256:");
    for byte in digest {
        use core::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}")
            .map_err(|_| p::Error("failed to encode execution digest".into()))?;
    }
    Ok(p::PlanDigest(encoded))
}
