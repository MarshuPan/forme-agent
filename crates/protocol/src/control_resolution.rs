//! Durable control resolutions for externally completed work.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

use crate::{ApprovalDecision, CancelRequest, Error, IdempotencyKey, Result, SchemaVersion};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunControl {
    ResolveApproval(ApprovalDecision),
    Cancel(CancelRequest),
    ResolveToolOutcome {
        command: IdempotencyKey,
        resolution: ToolOutcomeResolution,
    },
    ResolveHandoff {
        command: IdempotencyKey,
        resolution: HandoffResolutionRequest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolOutcomeResolution {
    pub schema_version: SchemaVersion,
    pub action: crate::ActionId,
    pub outcome: ToolOutcomeKind,
    pub result_ref: Option<crate::ActionResultRef>,
    pub failure_ref: Option<crate::FailureEvidenceRef>,
    pub reason: Option<crate::ReasonRef>,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolOutcomeKind {
    Completed,
    Failed,
    Cancelled,
}

impl ToolOutcomeResolution {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.action.0.trim().is_empty()
            || self.action.0.len() > 256
            || self.summary.len() > 4096
            || matches!(self.outcome, ToolOutcomeKind::Completed) && self.result_ref.is_none()
            || matches!(self.outcome, ToolOutcomeKind::Failed) && self.failure_ref.is_none()
            || matches!(self.outcome, ToolOutcomeKind::Cancelled) && self.reason.is_none()
        {
            return Err(Error("tool outcome resolution is invalid".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffResolutionRequest {
    pub schema_version: SchemaVersion,
    pub target: crate::HandoffTargetRef,
    pub accepted: bool,
    pub result: Option<String>,
}

impl HandoffResolutionRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.target.0.trim().is_empty()
            || self.target.0.len() > 256
            || self
                .result
                .as_ref()
                .is_some_and(|value| value.len() > 16_384)
        {
            return Err(Error("handoff resolution is invalid".into()));
        }
        Ok(())
    }
}

impl RunControl {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::ResolveApproval(value) => value.validate(),
            Self::Cancel(value) => value.validate(),
            Self::ResolveToolOutcome {
                command,
                resolution,
            } => {
                if command.0.trim().is_empty() {
                    return Err(Error("tool outcome command is incomplete".into()));
                }
                resolution.validate()
            }
            Self::ResolveHandoff {
                command,
                resolution,
            } => {
                if command.0.trim().is_empty() {
                    return Err(Error("handoff command is incomplete".into()));
                }
                resolution.validate()
            }
        }
    }
}
