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
                if !valid_command(command) {
                    return Err(Error("tool outcome command is incomplete".into()));
                }
                resolution.validate()
            }
            Self::ResolveHandoff {
                command,
                resolution,
            } => {
                if !valid_command(command) {
                    return Err(Error("handoff command is incomplete".into()));
                }
                resolution.validate()
            }
        }
    }
}

fn valid_command(command: &IdempotencyKey) -> bool {
    !command.0.trim().is_empty()
        && command.0.len() <= 256
        && !command.0.chars().any(char::is_control)
}

/// Reserved, versioned RunResumed encoding. It claims a command, not an action
/// outcome; a claim without its subsequent result must remain uncertain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlResolutionBinding {
    pub schema_version: SchemaVersion,
    pub command_digest: crate::SchemaDigest,
    pub resolution_digest: crate::SchemaDigest,
    pub target_digest: crate::SchemaDigest,
}

impl ControlResolutionBinding {
    pub fn from_ref(value: &crate::ResumeRef) -> Result<Option<Self>> {
        let Some(encoded) = value.0.strip_prefix("control-resolution-v1|") else {
            return Ok(None);
        };
        let mut fields = encoded.split('|');
        let Some(command) = fields.next() else {
            return Err(Error("control resolution binding is invalid".into()));
        };
        let Some(resolution) = fields.next() else {
            return Err(Error("control resolution binding is invalid".into()));
        };
        let Some(target) = fields.next() else {
            return Err(Error("control resolution binding is invalid".into()));
        };
        if fields.next().is_some() {
            return Err(Error("control resolution binding is invalid".into()));
        }
        let digests = [command, resolution, target].map(|value| crate::SchemaDigest(value.into()));
        for digest in &digests {
            crate::sha256_digest_bytes(digest)?;
        }
        Ok(Some(Self {
            schema_version: SchemaVersion(1),
            command_digest: digests[0].clone(),
            resolution_digest: digests[1].clone(),
            target_digest: digests[2].clone(),
        }))
    }

    pub fn as_ref(&self) -> crate::ResumeRef {
        crate::ResumeRef(format!(
            "control-resolution-v1|{}|{}|{}",
            self.command_digest.0, self.resolution_digest.0, self.target_digest.0
        ))
    }

    pub fn from_event(event: &crate::Event) -> Result<Option<Self>> {
        match &event.payload {
            crate::EventPayload::RunResumed(payload) => Self::from_ref(&payload.resume_ref),
            _ => Ok(None),
        }
    }
}
