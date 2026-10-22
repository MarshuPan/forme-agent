//! Closed owner workbench projections. These are adapters, not new brain facts.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkbenchSection {
    Memory,
    Cognition,
    Inbox,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchQuery {
    pub schema_version: SchemaVersion,
    pub section: WorkbenchSection,
    pub scope: Option<Scope>,
    pub offset: usize,
    pub limit: usize,
    pub snapshot: Option<SchemaDigest>,
}

impl WorkbenchQuery {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.limit == 0
            || self.limit > 100
            || self.offset > 1_000_000
            || self.offset > 0 && self.snapshot.is_none()
            || self.scope.as_ref().is_some_and(|scope| {
                scope.0.trim().is_empty()
                    || scope.0.len() > 512
                    || scope.0.chars().any(char::is_control)
            })
        {
            return Err(Error("workbench query is invalid or unbounded".into()));
        }
        if let Some(digest) = &self.snapshot {
            sha256_digest_bytes(digest)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkbenchItemKind {
    Memory,
    Candidate,
    Stable,
    Reevaluation,
    Suggestion,
    AskToLearn,
    Failure,
    Waiting,
}

/// Sequence is meaningful only inside `run`, never across this list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchItem {
    pub schema_version: SchemaVersion,
    pub id: String,
    pub kind: WorkbenchItemKind,
    pub run: RunId,
    pub event: EventId,
    pub stream_seq: u64,
    pub at: Timestamp,
    pub scope: Option<Scope>,
    pub state: String,
    pub summary: String,
    pub content: Option<ContentRef>,
    pub tier: Option<StabilityTier>,
    pub confidence: Option<Confidence>,
    pub evidence: Vec<EvidenceRef>,
    pub evidence_count: usize,
    pub derived: Vec<ObjectRef>,
    pub derived_count: usize,
    pub reviews: Vec<CandidateReviewDecision>,
    pub learning: Option<AskToLearnRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchSnapshot {
    pub schema_version: SchemaVersion,
    pub section: WorkbenchSection,
    pub digest: SchemaDigest,
    pub total: usize,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub items: Vec<WorkbenchItem>,
}

/// No actor, target, evidence or derived-object list can be supplied by a browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchReviewCommand {
    pub schema_version: SchemaVersion,
    pub command: IdempotencyKey,
    pub run: RunId,
    pub candidate: CandidateId,
    pub expected_state: CandidateReviewState,
    pub decision: CandidateReviewDecision,
}

impl WorkbenchReviewCommand {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) {
            return Err(Error("workbench review schema is unsupported".into()));
        }
        for value in [&self.command.0, &self.run.0, &self.candidate.0] {
            if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err(Error("workbench review identity is invalid".into()));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkbenchCommandStatus {
    Confirmed,
    Rejected,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchCommandReceipt {
    pub schema_version: SchemaVersion,
    pub command: IdempotencyKey,
    pub binding: SchemaDigest,
    pub status: WorkbenchCommandStatus,
    pub event: EventId,
}
