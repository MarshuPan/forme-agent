//! Owner feedback is an input to governance, never a stable-field patch.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkbenchCorrectionKind {
    Candidate,
    Stable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchCorrectionCommand {
    pub schema_version: SchemaVersion,
    pub command: IdempotencyKey,
    pub run: RunId,
    pub kind: WorkbenchCorrectionKind,
    pub id: String,
    pub event: EventId,
    pub snapshot: SchemaDigest,
    pub statement: String,
}

impl WorkbenchCorrectionCommand {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || [&self.command.0, &self.run.0, &self.id, &self.event.0]
                .iter()
                .any(|v| !valid_id(v))
            || self.statement.trim().is_empty()
            || self.statement.len() > 8_192
            || self
                .statement
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(Error("workbench correction is invalid or unbounded".into()));
        }
        sha256_digest_bytes(&self.snapshot)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchCorrectionQuery {
    pub schema_version: SchemaVersion,
    pub command: Option<IdempotencyKey>,
}

impl WorkbenchCorrectionQuery {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.command.as_ref().is_some_and(|v| !valid_id(&v.0))
        {
            return Err(Error("workbench correction query is invalid".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchCorrectionReceipt {
    pub schema_version: SchemaVersion,
    pub command: IdempotencyKey,
    pub binding: SchemaDigest,
    pub status: WorkbenchCommandStatus,
    pub event: EventId,
    pub candidate: CandidateId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchCorrectionsView {
    pub schema_version: SchemaVersion,
    pub receipts: Vec<WorkbenchCorrectionReceipt>,
    pub truncated: bool,
}

fn valid_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
